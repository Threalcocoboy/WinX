pub mod awareness;
pub mod device;
pub mod hotkeys;
pub mod media;
pub mod models;
pub mod preferences;
pub mod protocol;
pub mod talk;
pub mod ui_state;
pub mod updater;
#[cfg(windows)]
pub mod winx_update;

#[cfg(windows)]
pub const APP_NAME: &str = "WinX";
#[cfg(not(windows))]
pub const APP_NAME: &str = "NothingX Desktop";
