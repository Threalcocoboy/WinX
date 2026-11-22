use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Position {
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    #[default]
    BottomRight,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Size {
    Compact,
    #[default]
    Normal,
    Large,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct MediaOptions {
    pub play_pause: bool,
    pub track: bool,
    pub volume: bool,
}

impl Default for MediaOptions {
    fn default() -> Self {
        Self {
            play_pause: true,
            track: true,
            volume: true,
        }
    }
}

impl MediaOptions {
    pub fn enabled(self) -> bool {
        self.play_pause || self.track || self.volume
    }
}

fn load_media<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<MediaOptions, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Stored {
        Legacy(bool),
        Individual(MediaOptions),
    }
    Ok(match Stored::deserialize(deserializer)? {
        Stored::Legacy(enabled) => MediaOptions {
            play_pause: enabled,
            track: enabled,
            volume: enabled,
        },
        Stored::Individual(options) => options,
    })
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OverlayOptions {
    pub enabled: bool,
    pub connected: bool,
    pub disconnected: bool,
    pub noise: bool,
    pub controls: bool,
    #[serde(deserialize_with = "load_media")]
    pub media: MediaOptions,
    pub position: Position,
    pub monitor: String,
    pub seconds: u64,
    pub size: Size,
    pub appearance: Appearance,
}

impl Default for OverlayOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            connected: true,
            disconnected: true,
            noise: true,
            controls: true,
            media: MediaOptions::default(),
            position: Position::default(),
            monitor: "foreground".into(),
            seconds: 2,
            size: Size::default(),
            appearance: Appearance::default(),
        }
    }
}

impl OverlayOptions {
    pub fn valid(&self) -> bool {
        [1, 2, 3, 5].contains(&self.seconds)
            && (["foreground", "primary"].contains(&self.monitor.as_str())
                || (self.monitor.starts_with(r"\\.\DISPLAY") && self.monitor.len() <= 32))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BatteryOptions {
    pub enabled: bool,
    pub thresholds: [u8; 3],
    pub left: bool,
    pub right: bool,
    #[serde(rename = "case")]
    pub case_: bool,
    pub show_low: bool,
    pub show_critical: bool,
    pub combine: bool,
    pub quiet: bool,
    pub sound: bool,
    pub priority_seconds: u64,
}

impl Default for BatteryOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            thresholds: [20, 10, 5],
            left: true,
            right: true,
            case_: true,
            show_low: true,
            show_critical: true,
            combine: true,
            quiet: true,
            sound: false,
            priority_seconds: 5,
        }
    }
}

impl BatteryOptions {
    pub fn valid(&self) -> bool {
        let [low, very_low, critical] = self.thresholds;
        low <= 100
            && low > very_low
            && very_low > critical
            && [1, 2, 3, 5].contains(&self.priority_seconds)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Preferences {
    pub overlay: OverlayOptions,
    pub battery: BatteryOptions,
    pub hotkeys: crate::hotkeys::Bindings,
    pub talk: crate::talk::Options,
    pub settings_theme: Appearance,
    pub media_diagnostics: bool,
    pub onboarding_version: u32,
    pub talk_shortcut_initialized: bool,
    pub preferred_device: Option<String>,
}

impl Preferences {
    pub const TOUR_VERSION: u32 = 1;

    pub fn should_onboard(directory: &Path) -> bool {
        let path = directory.join("winx-preferences.json");
        if !path.exists() {
            return true;
        }
        let saved: serde_json::Value = load_json(&path);
        saved["onboarding_version"]
            .as_u64()
            .is_some_and(|version| version < u64::from(Self::TOUR_VERSION))
    }

    pub fn load(directory: &Path) -> Self {
        let path = directory.join("winx-preferences.json");
        let mut prefs: Self = load_json(&path);
        if path.exists() {
            let saved: serde_json::Value = load_json(&path);
            if saved.get("onboarding_version").is_none() {
                prefs.onboarding_version = Self::TOUR_VERSION;
            }
        }
        if path.exists() && !prefs.talk_shortcut_initialized {
            use crate::hotkeys::{Bindings, Shortcut};
            let chord = Bindings::default().keys[&Shortcut::TalkMode];
            if !prefs.hotkeys.keys.values().any(|&key| key == chord) {
                prefs
                    .hotkeys
                    .keys
                    .entry(Shortcut::TalkMode)
                    .or_insert(chord);
            }
        }
        prefs.talk_shortcut_initialized = true;
        if !prefs.talk.valid() {
            prefs.talk = crate::talk::Options::default();
        }
        if !prefs.overlay.valid() {
            prefs.overlay = OverlayOptions::default();
        }
        if !prefs.battery.valid() {
            prefs.battery = BatteryOptions::default();
        }
        prefs.hotkeys.normalize();
        prefs
    }

    pub fn save(&self, directory: &Path) -> io::Result<()> {
        save_json(&directory.join("winx-preferences.json"), self)
    }
}

pub fn load_json<T: Default + serde::de::DeserializeOwned>(path: &Path) -> T {
    fs::metadata(path)
        .ok()
        .filter(|m| m.len() <= 256_000)
        .and_then(|_| fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let temp = path.with_extension("tmp");
    fs::write(&temp, serde_json::to_vec(value)?)?;
    fs::rename(temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onboarding_and_talk_migration_preserve_existing_assignments() {
        use crate::hotkeys::Shortcut;
        use serde_json::json;
        let directory =
            std::env::temp_dir().join(format!("winx-onboarding-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("winx-preferences.json");
        assert!(Preferences::should_onboard(&directory));
        let fresh = Preferences::load(&directory);
        assert_eq!(fresh.talk.volume, 5);
        assert_eq!(fresh.talk.fade_ms, 600);
        assert!(fresh.talk.enabled);
        assert_eq!(fresh.hotkeys.keys[&Shortcut::TalkMode].vk, 72);
        fresh.save(&directory).unwrap();
        assert!(Preferences::should_onboard(&directory));

        save_json(&path, &json!({"hotkeys":{"keys":{"panel":{"modifiers":3,"vk":72}}},"overlay":{"connected":false}})).unwrap();
        assert!(!Preferences::should_onboard(&directory));
        let mut migrated = Preferences::load(&directory);
        assert_eq!(migrated.hotkeys.keys[&Shortcut::Panel].vk, 72);
        assert!(!migrated.hotkeys.keys.contains_key(&Shortcut::TalkMode));
        assert!(!migrated.overlay.connected);
        migrated.talk.volume = 8;
        migrated.save(&directory).unwrap();
        assert_eq!(Preferences::load(&directory).talk.volume, 8);
        assert!(!Preferences::should_onboard(&directory));
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }

    #[test]
    fn media_defaults_migrate_existing_choices_and_round_trip() {
        let default: OverlayOptions = serde_json::from_str("{}").unwrap();
        assert_eq!(
            default.media,
            MediaOptions {
                play_pause: true,
                track: true,
                volume: true
            }
        );
        let off: OverlayOptions = serde_json::from_str(r#"{"media":false}"#).unwrap();
        assert!(!off.media.enabled());
        let on: OverlayOptions = serde_json::from_str(r#"{"media":true}"#).unwrap();
        assert_eq!(on.media, default.media);
        let individual: OverlayOptions =
            serde_json::from_str(r#"{"media":{"track":false}}"#).unwrap();
        assert_eq!(
            individual.media,
            MediaOptions {
                play_pause: true,
                track: false,
                volume: true
            }
        );
        let saved = serde_json::to_string(&individual).unwrap();
        assert_eq!(
            serde_json::from_str::<OverlayOptions>(&saved)
                .unwrap()
                .media,
            individual.media
        );
    }

    #[test]
    fn connection_preferences_default_on_and_survive_upgrades_and_replacement() {
        let directory = std::env::temp_dir().join(format!(
            "winx-prefs-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("winx-preferences.json");
        fs::write(&path, r#"{"overlay":{"seconds":3,"position":"topLeft"}}"#).unwrap();
        let mut prefs = Preferences::load(&directory);
        assert!(prefs.overlay.connected && prefs.overlay.disconnected);
        assert_eq!(prefs.overlay.position, Position::TopLeft);
        prefs.overlay.disconnected = false;
        prefs.save(&directory).unwrap();
        let prefs = Preferences::load(&directory);
        assert!(prefs.overlay.connected);
        assert!(!prefs.overlay.disconnected);
        assert_eq!(prefs.overlay.seconds, 3);
        fs::write(&path, b"corrupt").unwrap();
        assert!(Preferences::load(&directory).overlay.disconnected);
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
