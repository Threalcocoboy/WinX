//! Opt-in, local media diagnostics. No device identifiers, playback metadata,
//! raw reports, ordinary keyboard input, or audio are written.
use std::{
    fs::OpenOptions,
    io::Write,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

static ENABLED: AtomicBool = AtomicBool::new(false);
pub fn enable(value: bool) {
    ENABLED.store(value, Ordering::Relaxed);
}
static FILE: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();

pub fn record(event: std::fmt::Arguments<'_>) {
    if !ENABLED.load(Ordering::Relaxed)
        && std::env::var("WINX_MEDIA_DIAGNOSTICS").as_deref() != Ok("1")
    {
        return;
    }
    let file = FILE.get_or_init(|| {
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(crate::platform::data_dir().join("winx-media-diagnostics.log"))
            .ok()
            .map(Mutex::new)
    });
    if let Some(file) = file
        && let Ok(mut file) = file.lock()
        && file.metadata().is_ok_and(|info| info.len() < 128 * 1024)
    {
        let _ = writeln!(file, "{event}");
    }
}
