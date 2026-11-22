use std::{fs, io, path::PathBuf};

#[cfg(windows)]
pub const WM_REOPEN: u32 = 0x8001; // WM_APP + 1

#[cfg(windows)]
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
#[cfg(windows)]
const RUN_NAME: &str = "WinX";

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

#[cfg(windows)]
fn startup_command() -> io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;

    let executable = std::env::current_exe()?;
    Ok(std::iter::once(u16::from(b'"'))
        .chain(executable.as_os_str().encode_wide())
        .chain(wide("\" --startup"))
        .collect())
}

#[cfg(windows)]
pub fn starts_with_windows() -> bool {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};

    let Ok(command) = startup_command() else {
        return false;
    };
    let mut stored = vec![0u16; command.len()];
    let mut size = (stored.len() * size_of::<u16>()) as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide(RUN_KEY).as_ptr(),
            wide(RUN_NAME).as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            stored.as_mut_ptr().cast(),
            &mut size,
        )
    };
    result == 0 && stored == command
}

#[cfg(windows)]
pub fn set_starts_with_windows(enabled: bool) -> io::Result<()> {
    use windows_sys::Win32::{
        Foundation::ERROR_FILE_NOT_FOUND,
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
            RegCreateKeyExW, RegDeleteValueW, RegSetValueExW,
        },
    };

    let command = startup_command()?;
    let mut key: HKEY = std::ptr::null_mut();
    let name = wide(RUN_NAME);
    let opened = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide(RUN_KEY).as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        )
    };
    if opened != 0 {
        return Err(io::Error::from_raw_os_error(opened as i32));
    }
    let result = unsafe {
        let result = if enabled {
            RegSetValueExW(
                key,
                name.as_ptr(),
                0,
                REG_SZ,
                command.as_ptr().cast(),
                (command.len() * size_of::<u16>()) as u32,
            )
        } else {
            RegDeleteValueW(key, name.as_ptr())
        };
        RegCloseKey(key);
        result
    };
    if result == 0 || (!enabled && result == ERROR_FILE_NOT_FOUND) {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(result as i32))
    }
}

#[cfg(windows)]
pub fn show_information(title: &str, body: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MessageBoxW};

    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide(body).as_ptr(),
            wide(title).as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

pub fn data_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        let base = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(std::env::temp_dir);
        base.join("Library/Application Support/NothingX Desktop")
    }
    #[cfg(windows)]
    {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| {
                std::env::var_os("USERPROFILE")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .map(|p| p.join("AppData/Local"))
            })
            .unwrap_or_else(std::env::temp_dir);
        base.join("NothingX Desktop")
    }
}

pub fn single_instance() -> io::Result<fs::File> {
    let dir = data_dir();
    fs::create_dir_all(&dir)?;
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("app.lock"))?;
    fs2::FileExt::try_lock_exclusive(&file)?;
    Ok(file)
}

pub fn open_url(url: &str) {
    if !url.starts_with("https://") || url.bytes().any(|b| b.is_ascii_control() || b == b' ') {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("/usr/bin/open").arg(url).spawn();
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .spawn();
    }
}

pub fn notify_running_instance() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            ASFW_ANY, AllowSetForegroundWindow, FindWindowW, PostMessageW,
        };
        let class: Vec<u16> = "NothingXDesktopWindow\0".encode_utf16().collect();
        let title = wide(nothingx_desktop::APP_NAME);
        unsafe {
            let hwnd = FindWindowW(class.as_ptr(), title.as_ptr());
            if !hwnd.is_null() {
                AllowSetForegroundWindow(ASFW_ANY);
                PostMessageW(hwnd, WM_REOPEN, 0, 0);
            }
        }
    }
}
