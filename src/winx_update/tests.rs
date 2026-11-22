use super::*;
use serde_json::json;
use std::io::Cursor;

fn release(version: &str, preview: bool) -> Value {
    let name = format!("WinX-v{version}-Setup-x64.exe");
    let base = format!("{RELEASES}/download/v{version}");
    json!({
        "tag_name": format!("v{version}"), "draft": false, "prerelease": preview,
        "html_url": format!("{RELEASES}/tag/v{version}"), "body": "Release information",
        "assets": [
            {"name": name, "size": 8000, "state": "uploaded", "browser_download_url": format!("{base}/{name}")},
            {"name": CHECKSUM, "size": 110, "state": "uploaded", "browser_download_url": format!("{base}/{CHECKSUM}")}
        ]
    })
}

#[test]
fn semantic_order_covers_stable_versions_and_prereleases() {
    for (candidate, current, newer) in [
        ("1.5.0", "1.5.0", false),
        ("1.5.1", "1.5.0", true),
        ("1.6.0", "1.5.0", true),
        ("2.0.0", "1.5.0", true),
        ("1.10.0", "1.9.0", true),
        ("1.5.0-beta.2", "1.5.0-beta.11", false),
        ("1.5.0", "1.5.0-beta.2", true),
        ("1.5.0-beta.2", "1.5.0", false),
        ("1.5.0+revision", "1.5.0", false),
        ("1.5", "1.5.0", false),
        ("", "1.5.0", false),
        ("1.6.0", "broken", false),
    ] {
        assert_eq!(
            is_newer(candidate, current),
            newer,
            "{candidate} vs {current}"
        );
    }
}

#[test]
fn release_selection_requires_official_metadata_and_channel_opt_in() {
    let stable = release("1.5.0", false);
    let preview = release("1.6.0-rc.2", true);
    assert!(parse_release(&preview, false, "1.5.0").unwrap().is_none());
    assert!(!is_newer(
        &parse_release(&stable, false, "1.5.0")
            .unwrap()
            .unwrap()
            .version,
        "1.5.0"
    ));
    let mut draft = release("9.0.0", false);
    draft["draft"] = json!(true);
    let body = json!([stable, preview, draft]).to_string();
    assert_eq!(
        select_release(body.as_bytes(), true, "1.5.0")
            .unwrap()
            .unwrap()
            .version,
        "1.6.0-rc.2"
    );
    assert!(select_release(b"not valid JSON", false, "1.5.0").is_err());
    assert!(select_release(b"{}", false, "1.5.0").is_err());
    assert!(select_release(b"[]", true, "1.5.0").unwrap().is_none());
    for wrong in [
        "WinX-v1.6.0-Setup-arm64.exe",
        "WinX-v1.6.0-Portable-x64.zip",
    ] {
        let mut metadata = release("1.6.0", false);
        metadata["assets"][0]["name"] = json!(wrong);
        assert!(parse_release(&metadata, false, "1.5.0").is_err());
    }
    let mut metadata = release("1.6.0", false);
    metadata["assets"][1]["browser_download_url"] =
        json!("https://github.com/third-party/WinX/releases/download/v1.6.0/SHA256SUMS.txt");
    assert!(parse_release(&metadata, false, "1.5.0").is_err());
    metadata["assets"][1] = Value::Null;
    assert!(parse_release(&metadata, false, "1.5.0").is_err());
    metadata["tag_name"] = Value::Null;
    assert!(parse_release(&metadata, false, "1.5.0").is_err());
}

#[test]
fn git_hub_headers_keep_body_and_reject_injected_etags_and_redirects() {
    let source = b"HTTP/2 304\r\nETag: W/\"known\"\r\n\r\nbody";
    let mut reader = Cursor::new(source);
    let (code, headers) = read_headers(&mut reader).unwrap();
    assert_eq!(code, 304);
    assert!(valid_etag(&headers[0].1));
    let mut body = String::new();
    reader.read_to_string(&mut body).unwrap();
    assert_eq!(body, "body");
    assert!(read_headers(&mut Cursor::new(b"")).is_err());
    assert!(read_headers(&mut Cursor::new(b"HTTP/2 200\r\nincomplete")).is_err());
    assert!(!valid_etag("\"first\r\nUser-Agent: injected\""));
    assert!(allowed_redirect(
        "https://release-assets.githubusercontent.com/path?signature=a%2Bb"
    ));
    for url in [
        "http://release-assets.githubusercontent.com/path",
        "https://release-assets.githubusercontent.com.evil.example/path",
        "https://release-assets.githubusercontent.com@evil.example/path",
        "https://user@release-assets.githubusercontent.com/path",
        "https://release-assets.githubusercontent.com:8443/path",
        "https://uploads.github.com/path",
    ] {
        assert!(!allowed_redirect(url), "{url}");
    }
}

#[test]
fn api_status_handles_cache_missing_release_and_rate_limits() {
    let now = 10_000;
    assert!(!use_cached(200, None, now, None, None).unwrap());
    assert!(use_cached(304, Some("\"valid\""), now, None, None).unwrap());
    assert!(use_cached(304, None, now, None, None).is_err());
    assert_eq!(
        use_cached(429, None, now, Some(120), None)
            .unwrap_err()
            .retry_at,
        Some(now + 120)
    );
    assert_eq!(
        use_cached(403, None, now, None, Some(now + 600))
            .unwrap_err()
            .retry_at,
        Some(now + 600)
    );
    assert!(
        use_cached(404, None, now, None, None)
            .unwrap_err()
            .message
            .contains("no published")
    );
    assert!(
        use_cached(503, None, now, None, None)
            .unwrap_err()
            .message
            .contains("503")
    );
}

#[test]
fn download_counts_real_bytes_and_handles_interruptions_and_write_failure() {
    let mut output = Vec::new();
    let mut received = Vec::new();
    copy_exact(&mut Cursor::new(b"abc"), &mut output, 3, |n| {
        received.push(n)
    })
    .unwrap();
    assert_eq!(output, b"abc");
    assert_eq!(received, [3]);
    assert!(copy_exact(&mut Cursor::new(b"ab"), &mut Vec::new(), 3, |_| {}).is_err());
    assert!(copy_exact(&mut Cursor::new(b"abcd"), &mut Vec::new(), 3, |_| {}).is_err());
    let mut storage = [0u8; 1];
    let mut full = Cursor::new(&mut storage[..]);
    assert!(
        copy_exact(&mut Cursor::new(b"abc"), &mut full, 3, |_| {})
            .unwrap_err()
            .contains("disk space")
    );
    assert!(copy_exact(&mut Cursor::new(b"abc"), &mut Vec::new(), 3, |_| {}).is_ok());
}

#[test]
fn checksum_needs_one_exact_valid_asset_name() {
    let name = "WinX-v1.6.0-Setup-x64.exe";
    let hash = "79".repeat(32);
    let row = format!("{hash} *{name}");
    assert_eq!(checksum(&row, name).as_deref(), Some(hash.as_str()));
    assert!(checksum(&format!("{row}\n{row}"), name).is_none());
    assert!(checksum(&format!("{hash} path/{name}"), name).is_none());
    assert!(checksum(&format!("wrong {name}"), name).is_none());
}

#[test]
fn invalid_executable_is_rejected_and_cancelled_download_is_removed() {
    let temp = std::env::temp_dir().join(format!(
        "winx-update-test-{}-{}",
        std::process::id(),
        epoch()
    ));
    let release = parse_release(&release("1.6.0", false), false, "1.5.0")
        .unwrap()
        .unwrap();
    let stage = staging(&temp, &release).unwrap();
    {
        fs::write(&stage.path, [0; 100]).unwrap();
        assert!(verify_image(&mut File::open(&stage.path).unwrap()).is_err());
        let mut image = [0; 128];
        image[..2].copy_from_slice(b"MZ");
        image[60] = 64;
        image[64..68].copy_from_slice(b"PE\0\0");
        image[68..70].copy_from_slice(&[0x4c, 0x01]);
        image[86] = 2;
        image[88..90].copy_from_slice(&[0x0b, 0x01]);
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .truncate(true)
            .open(&stage.path)
            .unwrap();
        file.write_all(&image).unwrap();
        let expected = format!("{:x}", Sha256::digest(image));
        assert_eq!(verify_installer(&mut file, &expected).unwrap(), expected);
        assert!(
            verify_installer(&mut file, &"0".repeat(64))
                .unwrap_err()
                .contains("SHA-256")
        );
        drop(file);
        image[68..70].copy_from_slice(&[0x64, 0xaa]);
        fs::write(&stage.path, image).unwrap();
        assert!(verify_image(&mut File::open(&stage.path).unwrap()).is_err());
    }
    let work = stage.work.clone();
    drop(stage);
    assert!(!work.exists());
    fs::remove_dir(temp.join("updates")).unwrap();
    fs::remove_dir(temp).unwrap();
}
