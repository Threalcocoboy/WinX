use crate::{
    device::{Action, Device, Key, Value},
    ui_state,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Shortcut {
    Panel,
    CycleNoise,
    NoiseOn,
    NoiseOff,
    Transparency,
    LowLatency,
    TalkMode,
    BassToggle,
    BassUp,
    BassDown,
    EqCycle,
    EqFirst,
    EqSecond,
    EqThird,
    EqFourth,
    EqFifth,
    EqSixth,
    EqSeventh,
    EqEighth,
    Find,
    RingLeft,
    RingRight,
    StopRing,
    Battery,
    Overlay,
}

pub const SHORTCUTS: &[(Shortcut, &str)] = &[
    (Shortcut::Panel, "Open or close WinX"),
    (Shortcut::CycleNoise, "Cycle noise control"),
    (Shortcut::NoiseOn, "Noise cancellation on"),
    (Shortcut::NoiseOff, "Noise cancellation off"),
    (Shortcut::Transparency, "Toggle transparency"),
    (Shortcut::LowLatency, "Toggle low latency"),
    (Shortcut::TalkMode, "Talk Mode"),
    (Shortcut::BassToggle, "Toggle bass enhancement"),
    (Shortcut::BassUp, "Increase bass"),
    (Shortcut::BassDown, "Decrease bass"),
    (Shortcut::EqCycle, "Cycle EQ presets"),
    (Shortcut::EqFirst, "Select EQ preset 1"),
    (Shortcut::EqSecond, "Select EQ preset 2"),
    (Shortcut::EqThird, "Select EQ preset 3"),
    (Shortcut::EqFourth, "Select EQ preset 4"),
    (Shortcut::EqFifth, "Select EQ preset 5"),
    (Shortcut::EqSixth, "Select EQ preset 6"),
    (Shortcut::EqSeventh, "Select EQ preset 7"),
    (Shortcut::EqEighth, "Select EQ preset 8"),
    (Shortcut::Find, "Ring earbuds"),
    (Shortcut::RingLeft, "Ring left earbud"),
    (Shortcut::RingRight, "Ring right earbud"),
    (Shortcut::StopRing, "Stop ringing"),
    (Shortcut::Battery, "Show battery"),
    (Shortcut::Overlay, "Toggle overlay"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Chord {
    // MOD_ALT = 1, MOD_CONTROL = 2, MOD_SHIFT = 4. The Windows key is reserved.
    pub modifiers: u32,
    pub vk: u32,
}

impl Chord {
    pub fn valid(self) -> bool {
        self.modifiers & !7 == 0 && self.modifiers & 3 != 0
            && matches!(self.vk, 0x20 | 0x21..=0x28 | 0x30..=0x39 | 0x41..=0x5a | 0x70..=0x7b)
            && self.vk != 0x7b // F12 is reserved by Windows for debugging.
            && !(self.modifiers & 1 != 0 && self.modifiers & 2 == 0 && matches!(self.vk, 0x20 | 0x73))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Bindings {
    pub enabled: bool,
    pub keys: BTreeMap<Shortcut, Chord>,
}

impl Default for Bindings {
    fn default() -> Self {
        Self {
            enabled: true,
            keys: [
                (Shortcut::Panel, b'X'),
                (Shortcut::CycleNoise, b'N'),
                (Shortcut::Transparency, b'T'),
                (Shortcut::LowLatency, b'G'),
                (Shortcut::TalkMode, b'H'),
            ]
            .into_iter()
            .map(|(action, key)| {
                (
                    action,
                    Chord {
                        modifiers: 3,
                        vk: u32::from(key),
                    },
                )
            })
            .collect(),
        }
    }
}

impl Bindings {
    pub fn normalize(&mut self) {
        let mut seen = BTreeSet::new();
        self.keys.retain(|_, key| key.valid() && seen.insert(*key));
    }
}

impl Shortcut {
    pub fn preset_slot(self) -> Option<usize> {
        use Shortcut::*;
        Some(match self {
            EqFirst => 0,
            EqSecond => 1,
            EqThird => 2,
            EqFourth => 3,
            EqFifth => 4,
            EqSixth => 5,
            EqSeventh => 6,
            EqEighth => 7,
            _ => return None,
        })
    }

    pub fn device_action(self, d: &Device) -> Option<Action> {
        use Shortcut::*;
        let caps = d.caps.as_ref().filter(|_| d.connected && d.received_any)?;
        let action = match self {
            CycleNoise | NoiseOn | NoiseOff | Transparency => {
                let current = match d.value(Key::Noise)? {
                    Value::Byte(n) => *n,
                    _ => return None,
                };
                let mode = match self {
                    NoiseOn if caps.noise_cancellation => "anc",
                    NoiseOff if caps.noise_cancellation || caps.transparency => "off",
                    Transparency if caps.transparency && current != 7 => "transparency",
                    Transparency if caps.transparency && caps.noise_cancellation => "anc",
                    Transparency if caps.transparency => "off",
                    CycleNoise if (1..=4).contains(&current) && caps.transparency => "transparency",
                    CycleNoise if current == 5 && caps.noise_cancellation => "anc",
                    CycleNoise if current == 5 && caps.transparency => "transparency",
                    CycleNoise if caps.noise_cancellation || caps.transparency => "off",
                    _ => return None,
                };
                return ui_state::command(d, "setNoiseMode", &[json!(mode)]);
            }
            LowLatency if caps.low_latency => {
                let Value::Flag(on) = d.value(Key::LowLatency)? else {
                    return None;
                };
                Action::Set(Key::LowLatency, Value::Flag(!on))
            }
            BassToggle | BassUp | BassDown if caps.bass_steps > 0 => {
                let Value::Pair(on, wire) = d.value(Key::Bass)? else {
                    return None;
                };
                let (step, max) = if caps.bass_steps == 3 { (5, 2) } else { (2, 5) };
                let level = (wire / step).clamp(1, max);
                let change = match self {
                    BassToggle => json!({"enabled": *on == 0, "level": level}),
                    BassUp => json!({"enabled": true, "level": (level + 1).min(max)}),
                    _ => json!({"enabled": true, "level": level.saturating_sub(1).max(1)}),
                };
                return ui_state::command(d, "setBass", &[change]);
            }
            EqCycle => {
                let Value::Byte(id) = d.value(Key::Preset)? else {
                    return None;
                };
                let presets = &caps.eq.presets;
                let next = presets
                    .iter()
                    .position(|preset| preset.id == *id)
                    .and_then(|index| presets.get(index + 1))
                    .or_else(|| presets.first())?;
                Action::Set(Key::Preset, Value::Byte(next.id))
            }
            Find if caps.ring != "none" => Action::Ring(6),
            RingLeft | RingRight if caps.ring == "buds" => {
                Action::Ring(if self == RingLeft { 2 } else { 3 })
            }
            StopRing if d.ringing.is_some() => Action::StopRing,
            _ => Action::Set(
                Key::Preset,
                Value::Byte(caps.eq.presets.get(self.preset_slot()?)?.id),
            ),
        };
        Some(action)
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::ptr::null_mut;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey,
    };

    #[derive(Default)]
    pub struct Hotkeys {
        registered: BTreeMap<Shortcut, (i32, Chord)>,
        counter: i32,
    }

    impl Hotkeys {
        pub fn register(&mut self, action: Shortcut, key: Chord) -> bool {
            if self
                .registered
                .get(&action)
                .is_some_and(|entry| entry.1 == key)
            {
                return true;
            }
            self.counter = (self.counter % 0x3fff) + 1;
            while self
                .registered
                .values()
                .any(|entry| entry.0 == self.counter)
            {
                self.counter = (self.counter % 0x3fff) + 1;
            }
            if unsafe {
                RegisterHotKey(
                    null_mut(),
                    self.counter,
                    key.modifiers | MOD_NOREPEAT,
                    key.vk,
                )
            } == 0
            {
                return false;
            }
            self.remove(action);
            self.registered.insert(action, (self.counter, key));
            true
        }

        pub fn remove(&mut self, action: Shortcut) {
            if let Some((id, _)) = self.registered.remove(&action) {
                unsafe {
                    UnregisterHotKey(null_mut(), id);
                }
            }
        }

        pub fn sync(&mut self, bindings: &Bindings) {
            for &(action, _) in SHORTCUTS {
                if let Some(&key) = bindings.keys.get(&action).filter(|_| bindings.enabled) {
                    if !self.register(action, key) {
                        self.remove(action);
                    }
                } else {
                    self.remove(action);
                }
            }
        }

        pub fn action(&self, id: i32) -> Option<Shortcut> {
            self.registered
                .iter()
                .find_map(|(&action, &(candidate, _))| (candidate == id).then_some(action))
        }

        pub fn available(&self, action: Shortcut) -> bool {
            self.registered.contains_key(&action)
        }
    }

    impl Drop for Hotkeys {
        fn drop(&mut self) {
            for &(action, _) in SHORTCUTS {
                self.remove(action);
            }
        }
    }
}

#[cfg(windows)]
pub use native::Hotkeys;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_chords_round_trip_and_reserved_combinations_are_rejected() {
        let expected = Bindings::default();
        let serialized = serde_json::to_string(&expected).unwrap();
        let restored: Bindings = serde_json::from_str(&serialized).unwrap();
        assert_eq!(restored.keys, expected.keys);
        assert!(restored.keys.values().all(|chord| chord.valid()));
        assert!(
            !Chord {
                modifiers: 8,
                vk: 88
            }
            .valid()
        );
        assert!(
            !Chord {
                modifiers: 1,
                vk: 0x73
            }
            .valid()
        );
        assert!(
            !Chord {
                modifiers: 3,
                vk: 0x7b
            }
            .valid()
        );
    }

    #[test]
    fn hardware_actions_only_use_confirmed_supported_controls() {
        let mut d = Device::default();
        assert!(Shortcut::CycleNoise.device_action(&d).is_none());
        d.on_connected("Nothing Ear (3)".into(), "0".into());
        assert!(Shortcut::EqFirst.device_action(&d).is_none());
        d.received_any = true;
        d.values.insert(Key::Noise, Value::Byte(1));
        assert!(matches!(
            Shortcut::CycleNoise.device_action(&d),
            Some(Action::Set(Key::Noise, Value::Byte(7)))
        ));
        d.on_connected("Ear (open)".into(), "1".into());
        d.received_any = true;
        assert!(Shortcut::NoiseOn.device_action(&d).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn windows_registration_checks_conflicts_reassigns_and_releases() {
        // Exercise the actual API on this test thread without posting any
        // hotkey messages or generating input.
        let mut keys = Hotkeys::default();
        let combinations = (0x76..=0x7a).rev().map(|vk| Chord { modifiers: 7, vk });
        let mut available = combinations
            .filter(|&chord| {
                let usable = keys.register(Shortcut::Panel, chord);
                keys.remove(Shortcut::Panel);
                usable
            })
            .take(2)
            .collect::<Vec<_>>();
        assert_eq!(available.len(), 2);
        let original = available.pop().unwrap();
        let replacement = available.pop().unwrap();
        assert!(keys.register(Shortcut::Panel, original));
        assert!(!keys.register(Shortcut::Battery, original));
        assert!(keys.available(Shortcut::Panel));
        assert!(keys.register(Shortcut::Panel, replacement));
        assert!(keys.register(Shortcut::Battery, original));
        drop(keys);
        let mut after = Hotkeys::default();
        assert!(after.register(Shortcut::Panel, replacement));
        assert!(after.register(Shortcut::Battery, original));
    }
}
