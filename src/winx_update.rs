//! WinX's Windows x64 release feed and installer verification.
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};
use url::Url;

pub const REPOSITORY: &str = "https://github.com/Threalcocoboy/WinX";
pub const RELEASES: &str = "https://github.com/Threalcocoboy/WinX/releases";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
pub const DAY: u64 = 86_400;
const API: &str = "https://api.github.com/repos/Threalcocoboy/WinX/releases";
const CHECKSUM: &str = "SHA256SUMS.txt";
const MAX_INSTALLER: u64 = 200 * 1024 * 1024;
const MAX_METADATA: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    pub page: String,
    pub notes: String,
    name: String,
    installer: String,
    pub size: u64,
    checksum: String,
    checksum_size: u64,
    #[serde(default)]
    github_digest: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Options {
    pub automatic: bool,
    pub notify: bool,
    pub prereleases: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            automatic: true,
            notify: true,
            prereleases: false,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Cache {
    pub checked: u64,
    pub prereleases: bool,
    pub release: Option<Release>,
    etag: Option<String>,
}

#[derive(Debug)]
pub struct CheckError {
    pub message: String,
    pub retry_at: Option<u64>,
}

impl From<String> for CheckError {
    fn from(message: String) -> Self {
        Self {
            message,
            retry_at: None,
        }
    }
}

pub fn epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    Version::parse(candidate)
        .ok()
        .zip(Version::parse(current).ok())
        .is_some_and(|(candidate, current)| candidate.cmp_precedence(&current).is_gt())
}

fn parse_release(
    value: &Value,
    prereleases: bool,
    current: &str,
) -> Result<Option<Release>, String> {
    if value["draft"]
        .as_bool()
        .ok_or("The release has no publication metadata.")?
    {
        return Ok(None);
    }
    let tag = value["tag_name"]
        .as_str()
        .ok_or("The release is missing its version.")?;
    let version = tag
        .strip_prefix('v')
        .ok_or("The release tag must start with v.")?;
    let parsed = Version::parse(version).map_err(|_| "The release has an invalid version.")?;
    if !parsed.build.is_empty() {
        return Err("WinX release tags cannot contain build metadata.".into());
    }
    let is_pre = value["prerelease"]
        .as_bool()
        .ok_or("The release has no channel metadata.")?;
    if is_pre == parsed.pre.is_empty() || (is_pre && !prereleases) {
        return Ok(None);
    }
    let name = format!("WinX-{tag}-Setup-x64.exe");
    let page = format!("{RELEASES}/tag/{tag}");
    if value["html_url"].as_str() != Some(page.as_str()) {
        return Err("The release has an unexpected GitHub page.".into());
    }
    let asset = |wanted: &str, max: u64| -> Option<(String, u64, Option<String>)> {
        let url = format!("{RELEASES}/download/{tag}/{wanted}");
        let mut matches = value["assets"]
            .as_array()?
            .iter()
            .filter(|a| a["name"] == wanted);
        let a = matches.next()?;
        if matches.next().is_some()
            || a["browser_download_url"].as_str()? != url
            || a["state"] != "uploaded"
        {
            return None;
        }
        let size = a["size"].as_u64()?;
        let digest = match a["digest"].as_str() {
            Some(digest) => {
                let hash = digest.strip_prefix("sha256:")?;
                if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return None;
                }
                Some(hash.to_ascii_lowercase())
            }
            None => None,
        };
        (size > 0 && size <= max).then_some((url, size, digest))
    };
    let (Some((installer, size, github_digest)), Some((checksum, checksum_size, _))) =
        (asset(&name, MAX_INSTALLER), asset(CHECKSUM, 65_536))
    else {
        return if is_newer(version, current) {
            Err(
                "The release is missing a compatible Windows x64 installer or checksum file."
                    .into(),
            )
        } else {
            Ok(None)
        };
    };
    Ok(Some(Release {
        version: version.into(),
        page,
        notes: value["body"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(32_000)
            .collect(),
        name,
        installer,
        size,
        checksum,
        checksum_size,
        github_digest,
    }))
}

fn select_release(
    body: &[u8],
    prereleases: bool,
    current: &str,
) -> Result<Option<Release>, String> {
    let json: Value =
        serde_json::from_slice(body).map_err(|_| "GitHub returned invalid release metadata.")?;
    if !prereleases {
        return parse_release(&json, false, current);
    }
    let items = json
        .as_array()
        .ok_or("GitHub returned an invalid release list.")?;
    let mut best: Option<Release> = None;
    let mut error = None;
    for item in items {
        match parse_release(item, true, current) {
            Ok(Some(next))
                if best
                    .as_ref()
                    .is_none_or(|old| is_newer(&next.version, &old.version)) =>
            {
                best = Some(next)
            }
            Err(message) => {
                error.get_or_insert(message);
            }
            _ => {}
        }
    }
    if best
        .as_ref()
        .is_none_or(|item| !is_newer(&item.version, current))
        && let Some(message) = error
    {
        return Err(message);
    }
    Ok(best)
}

fn system_program(relative: &str) -> Command {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let mut command = Command::new(PathBuf::from(root).join("System32").join(relative));
    command.creation_flags(0x08000000);
    command
}

struct Request {
    process: Child,
    body: BufReader<std::process::ChildStdout>,
    status: u16,
    headers: Vec<(String, String)>,
}

impl Request {
    fn get(url: &str, etag: Option<&str>, seconds: &str) -> Result<Self, String> {
        let mut command = system_program("curl.exe");
        command.args([
            "--disable",
            "--silent",
            "--include",
            "--suppress-connect-headers",
            "--proto",
            "=https",
            "--connect-timeout",
            "10",
            "--max-time",
            seconds,
            "--user-agent",
            concat!("WinX/", env!("CARGO_PKG_VERSION")),
        ]);
        if url.starts_with("https://api.github.com/") {
            command.args(["--header", "Accept: application/vnd.github+json"]);
        }
        if let Some(etag) = etag.filter(|value| valid_etag(value)) {
            command.args(["--header", &format!("If-None-Match: {etag}")]);
        }
        let mut process = command
            .arg("--")
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "Could not start the Windows HTTPS client.")?;
        let mut request = Self {
            body: BufReader::new(process.stdout.take().expect("piped stdout")),
            process,
            status: 0,
            headers: Vec::new(),
        };
        let (status, headers) = read_headers(&mut request.body)?;
        request.status = status;
        request.headers = headers;
        Ok(request)
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn finish(&mut self) -> Result<(), String> {
        match self.process.wait() {
            Ok(status) if status.success() => Ok(()),
            _ => {
                Err("The connection ended before GitHub completed the response. Try again.".into())
            }
        }
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        if self.process.try_wait().ok().flatten().is_none() {
            let _ = self.process.kill();
            let _ = self.process.wait();
        }
    }
}

fn read_headers(reader: &mut impl BufRead) -> Result<(u16, Vec<(String, String)>), String> {
    let mut limit = reader.take(32_768);
    let mut line = String::new();
    let bad = "GitHub could not be reached or returned an invalid HTTPS response.";
    limit.read_line(&mut line).map_err(|_| bad)?;
    let mut parts = line.split_whitespace();
    let status = parts
        .next()
        .filter(|p| p.starts_with("HTTP/"))
        .and_then(|_| parts.next())
        .and_then(|p| p.parse::<u16>().ok())
        .ok_or(bad)?;
    let mut headers = Vec::new();
    loop {
        line.clear();
        if limit.read_line(&mut line).map_err(|_| bad)? == 0 {
            return Err(bad.into());
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        let (name, value) = line.split_once(':').ok_or(bad)?;
        headers.push((name.to_ascii_lowercase(), value.trim().to_owned()));
    }
    Ok((status, headers))
}

fn valid_etag(value: &str) -> bool {
    let value = value.strip_prefix("W/").unwrap_or(value);
    value.len() <= 260
        && value
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .is_some_and(|part| part.bytes().all(|b| b.is_ascii_graphic() && b != b'"'))
}

fn use_cached(
    status: u16,
    etag: Option<&str>,
    now: u64,
    retry: Option<u64>,
    reset: Option<u64>,
) -> Result<bool, CheckError> {
    match status {
        304 if etag.is_some() => Ok(true),
        200 => Ok(false),
        404 => Err("GitHub has no published WinX release on this channel."
            .to_owned()
            .into()),
        403 | 429 => Err(CheckError {
            message: "GitHub is limiting update checks. Try again after the cooldown.".into(),
            retry_at: Some(
                retry
                    .map(|seconds| now.saturating_add(seconds))
                    .or(reset)
                    .unwrap_or(now + 3600)
                    .clamp(now + 60, now + DAY * 7),
            ),
        }),
        code => Err(format!("GitHub returned HTTP {code}. Try again later.").into()),
    }
}

pub fn check(prereleases: bool, cache: &Cache) -> Result<Cache, CheckError> {
    let endpoint = if prereleases {
        format!("{API}?per_page=100")
    } else {
        format!("{API}/latest")
    };
    let etag = (cache.prereleases == prereleases)
        .then_some(cache.etag.as_deref())
        .flatten()
        .filter(|value| valid_etag(value));
    let mut request = Request::get(&endpoint, etag, "25")?;
    let checked = epoch();
    let header_number = |name: &str| {
        request
            .header(name)
            .and_then(|value| value.parse::<u64>().ok())
    };
    if use_cached(
        request.status,
        etag,
        checked,
        header_number("retry-after"),
        header_number("x-ratelimit-reset"),
    )? {
        request.finish()?;
        return Ok(Cache {
            checked,
            ..cache.clone()
        });
    }
    let etag = request
        .header("etag")
        .filter(|v| valid_etag(v))
        .map(str::to_owned);
    let mut bytes = Vec::new();
    request
        .body
        .by_ref()
        .take(MAX_METADATA + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read the release response.".to_owned())?;
    if bytes.len() as u64 > MAX_METADATA {
        return Err("GitHub returned too much release data.".to_owned().into());
    }
    request.finish()?;
    let release = select_release(&bytes, prereleases, CURRENT)?;
    if release.is_none() {
        return Err(
            "GitHub has no compatible published WinX release on this channel."
                .to_owned()
                .into(),
        );
    }
    Ok(Cache {
        checked,
        prereleases,
        release,
        etag,
    })
}

fn official_asset(url: &str, name: &str, maximum: u64, size: u64) -> bool {
    let prefix = format!("{RELEASES}/download/v");
    url.strip_prefix(&prefix)
        .and_then(|path| path.split_once('/'))
        .is_some_and(|(tag, asset)| {
            Version::parse(tag).is_ok_and(|v| v.build.is_empty())
                && asset == name
                && (1..=maximum).contains(&size)
        })
}

fn allowed_redirect(destination: &str) -> bool {
    Url::parse(destination).is_ok_and(|url| {
        url.scheme() == "https"
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none()
            && matches!(
                url.host_str(),
                Some("release-assets.githubusercontent.com" | "objects.githubusercontent.com")
            )
            && destination.bytes().all(|b| b.is_ascii_graphic())
    })
}

fn asset_request(url: &str) -> Result<Request, String> {
    let mut request = Request::get(url, None, "300")?;
    if matches!(request.status, 301 | 302 | 303 | 307 | 308) {
        let destination = request
            .header("location")
            .filter(|v| allowed_redirect(v))
            .ok_or("GitHub returned an untrusted download redirect.")?
            .to_owned();
        drop(request);
        request = Request::get(&destination, None, "300")?;
    }
    if request.status != 200 {
        return Err(format!(
            "GitHub rejected the download (HTTP {}). Try checking again.",
            request.status
        ));
    }
    Ok(request)
}

fn copy_exact(
    input: &mut impl Read,
    output: &mut impl Write,
    size: u64,
    mut progress: impl FnMut(u64),
) -> Result<(), String> {
    let mut total = 0;
    let mut buffer = [0u8; 65_536];
    loop {
        let count = input
            .read(&mut buffer)
            .map_err(|_| "The download was interrupted.")?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > size {
            return Err("The download exceeded the published file size.".into());
        }
        output.write_all(&buffer[..count]).map_err(
            |_| "Could not save the download. Check available disk space and permissions.",
        )?;
        progress(total);
    }
    if total != size {
        return Err("The download ended before its published file size.".into());
    }
    Ok(())
}

fn checksum(text: &str, name: &str) -> Option<String> {
    let mut found = None;
    for line in text.lines() {
        let mut fields = line.split_ascii_whitespace();
        let (Some(hash), Some(file)) = (fields.next(), fields.next()) else {
            continue;
        };
        if file.strip_prefix('*').unwrap_or(file) != name {
            continue;
        }
        if found.is_some()
            || fields.next().is_some()
            || hash.len() != 64
            || !hash.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return None;
        }
        found = Some(hash.to_ascii_lowercase());
    }
    found
}

fn verify_image(file: &mut File) -> io::Result<()> {
    file.rewind()?;
    let mut dos = [0; 64];
    file.read_exact(&mut dos)?;
    let offset = u64::from(u32::from_le_bytes(dos[60..64].try_into().unwrap()));
    if dos[..2] != *b"MZ" || offset < 64 {
        return Err(io::ErrorKind::InvalidData.into());
    }
    file.seek(SeekFrom::Start(offset))?;
    let mut pe = [0; 26];
    file.read_exact(&mut pe)?;
    // NSIS distributions can use a 32-bit setup stub to install an x64 payload.
    let architecture = (&pe[4..6], &pe[24..26]);
    if pe[..4] != *b"PE\0\0"
        || !matches!(
            architecture,
            ([0x64, 0x86], [0x0b, 0x02]) | ([0x4c, 0x01], [0x0b, 0x01])
        )
        || pe[22] & 0x02 == 0
        || pe[23] & 0x20 != 0
    {
        return Err(io::ErrorKind::InvalidData.into());
    }
    file.rewind()
}

fn verify_installer(file: &mut File, expected: &str) -> Result<String, String> {
    verify_image(file)
        .map_err(|_| "The downloaded file is not a valid Windows setup executable.")?;
    let mut hash = Sha256::new();
    io::copy(file, &mut hash).map_err(|_| "Could not read the saved installer.")?;
    let digest = format!("{:x}", hash.finalize());
    if digest != expected {
        return Err("The installer SHA-256 did not match GitHub's checksum.".into());
    }
    Ok(digest)
}

#[derive(Debug)]
pub struct Verified {
    work: PathBuf,
    path: PathBuf,
    digest: String,
    version: String,
    handed_off: bool,
}

impl Drop for Verified {
    fn drop(&mut self) {
        if !self.handed_off {
            let _ = fs::remove_file(&self.path);
            let _ = fs::remove_dir(&self.work);
        }
    }
}

fn staging(directory: &Path, release: &Release) -> Result<Verified, String> {
    let base = directory.join("updates");
    fs::create_dir_all(&base).map_err(|_| "Could not create the local update folder.")?;
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let work = base.join(format!("{}-{id}", std::process::id()));
    fs::create_dir(&work).map_err(|_| "Could not reserve a new update folder.")?;
    Ok(Verified {
        path: work.join(&release.name),
        work,
        digest: String::new(),
        version: release.version.clone(),
        handed_off: false,
    })
}

pub fn download(
    release: &Release,
    directory: &Path,
    mut progress: impl FnMut(u64),
) -> Result<Verified, String> {
    let expected_name = format!("WinX-v{}-Setup-x64.exe", release.version);
    if !cfg!(target_arch = "x86_64")
        || !is_newer(&release.version, CURRENT)
        || release.name != expected_name
        || release.installer != format!("{RELEASES}/download/v{}/{expected_name}", release.version)
        || release.checksum != format!("{RELEASES}/download/v{}/{CHECKSUM}", release.version)
        || !official_asset(
            &release.installer,
            &expected_name,
            MAX_INSTALLER,
            release.size,
        )
        || !official_asset(&release.checksum, CHECKSUM, 65_536, release.checksum_size)
    {
        return Err("This release is not a newer WinX Windows x64 release.".into());
    }
    let mut checksums = asset_request(&release.checksum)?;
    let mut text = Vec::new();
    copy_exact(
        &mut checksums.body,
        &mut text,
        release.checksum_size,
        |_| {},
    )?;
    checksums.finish()?;
    let expected = std::str::from_utf8(&text)
        .ok()
        .and_then(|text| checksum(text, &release.name))
        .ok_or("The published checksum file does not identify this installer.")?;
    if release
        .github_digest
        .as_ref()
        .is_some_and(|digest| digest != &expected)
    {
        return Err("GitHub's asset digest and the published checksum disagree.".into());
    }

    let mut staged = staging(directory, release)?;
    let free =
        fs2::available_space(&staged.work).map_err(|_| "Could not check available disk space.")?;
    if free < release.size + 16 * 1024 * 1024 {
        return Err("There is not enough disk space to download this update.".into());
    }
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&staged.path)
        .map_err(|_| "Could not create the update file.")?;
    let mut request = asset_request(&release.installer)?;
    let mut previous_percent = 0;
    copy_exact(&mut request.body, &mut file, release.size, |received| {
        let percent = received * 100 / release.size;
        if percent != previous_percent {
            previous_percent = percent;
            progress(received);
        }
    })?;
    request.finish()?;
    progress(release.size);
    file.sync_all()
        .map_err(|_| "Could not save the complete installer.")?;
    staged.digest = verify_installer(&mut file, &expected)?;
    Ok(staged)
}

impl Verified {
    pub fn launch(&mut self) -> Result<(), String> {
        let current =
            std::env::current_exe().map_err(|_| "Could not locate the running WinX executable.")?;
        system_program("WindowsPowerShell/v1.0/powershell.exe")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                include_str!("update-install.ps1"),
            ])
            .env("WINX_UPDATE_OLD_PID", std::process::id().to_string())
            .env("WINX_UPDATE_CURRENT", current)
            .env("WINX_UPDATE_SETUP", &self.path)
            .env("WINX_UPDATE_DIGEST", &self.digest)
            .env("WINX_UPDATE_VERSION", &self.version)
            .env_remove("PSModulePath")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(
                |_| "Could not start the update installer helper. Retry or open GitHub Releases.",
            )?;
        self.handed_off = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
