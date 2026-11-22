//! Talk Mode uses confirmed device settings and restores only values it owns.
use crate::device::{Device, Key, Value};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Options {
    pub enabled: bool,
    pub volume: u8,
    pub transparency: bool,
    pub reduce_eq: bool,
    pub overlay: bool,
    pub fade_ms: u16,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            enabled: true,
            volume: 5,
            transparency: true,
            reduce_eq: true,
            overlay: true,
            fade_ms: 600,
        }
    }
}

impl Options {
    pub fn valid(self) -> bool {
        self.volume <= 100 && (100..=2_000).contains(&self.fade_ms)
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct DeviceSnapshot {
    pub address: String,
    pub changes: Vec<Change>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Change {
    pub key: Key,
    pub original: Value,
    pub target: Value,
    pub attempted: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Restore {
    Original,
    Owned,
    Changed,
    Waiting,
}

impl Change {
    pub fn restore(&self, device: &Device) -> Restore {
        if device.is_pending(self.key) {
            return Restore::Waiting;
        }
        match device.value(self.key) {
            Some(value) if *value == self.original => Restore::Original,
            Some(value) if *value == self.target => Restore::Owned,
            Some(_) => Restore::Changed,
            None => Restore::Waiting,
        }
    }
}

impl DeviceSnapshot {
    pub fn capture(device: &Device, options: Options) -> (Self, Vec<&'static str>) {
        let mut snapshot = Self {
            address: device.address.clone(),
            ..Self::default()
        };
        let mut skipped = Vec::new();
        if options.reduce_eq {
            let eq = device.caps.as_ref().map(|caps| &caps.eq);
            let start = snapshot.changes.len();
            let captured = device.exact_custom_eq()
                && eq
                    .and_then(|eq| eq.custom_id.zip(eq.gain_range()))
                    .is_some_and(|(id, range)| {
                        // Always require both confirmed values, even if Custom is already selected.
                        snapshot.add(device, Key::Preset, Value::Byte(id))
                            && snapshot.add(
                                device,
                                Key::CustomEq,
                                Value::Eq([*range.start(), *range.end(), *range.start()]),
                            )
                    });
            if !captured {
                snapshot.changes.truncate(start);
                skipped.push("EQ unavailable");
            }
        }
        if options.transparency && !snapshot.add(device, Key::Noise, Value::Byte(7)) {
            skipped.push("Transparency unavailable");
        }
        (snapshot, skipped)
    }

    fn add(&mut self, device: &Device, key: Key, target: Value) -> bool {
        let Some(original) = device.value(key) else {
            return false;
        };
        if device.is_pending(key) || !device.can_set(key, original) || !device.can_set(key, &target)
        {
            return false;
        }
        if *original != target {
            self.changes.push(Change {
                key,
                original: original.clone(),
                target,
                attempted: false,
            });
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_preserves_exact_anc_preset_and_bands() {
        let mut device = Device::default();
        device.on_connected("Nothing Ear (3)".into(), "001122334455".into());
        let preset = device.caps.as_ref().unwrap().eq.presets[0].id;
        device.values.insert(Key::Noise, Value::Byte(1));
        device.values.insert(Key::Preset, Value::Byte(preset));
        let eq = &device.caps.as_ref().unwrap().eq;
        let payload = crate::device::encode_eq([3, -4, 2], eq.treble_hz);
        device.receive(&crate::protocol::encode(0x4044, &payload, 1).unwrap());
        let (snapshot, skipped) = DeviceSnapshot::capture(&device, Options::default());
        assert!(skipped.is_empty());
        assert_eq!(snapshot.changes.len(), 3);
        assert_eq!(snapshot.changes[2].original, Value::Byte(1));
        assert_eq!(snapshot.changes[0].original, Value::Byte(preset));
        assert_eq!(snapshot.changes[1].original, Value::Eq([3, -4, 2]));
        assert_eq!(snapshot.changes[1].target, Value::Eq([-6, 6, -6]));
        assert_eq!(snapshot.changes[1].restore(&device), Restore::Original);
        device.values.insert(Key::CustomEq, Value::Eq([-6, 6, -6]));
        assert_eq!(snapshot.changes[1].restore(&device), Restore::Owned);
        device.values.insert(Key::CustomEq, Value::Eq([1, 2, 3]));
        assert_eq!(snapshot.changes[1].restore(&device), Restore::Changed);
    }

    #[test]
    fn skips_eq_when_either_original_cannot_be_captured() {
        let mut device = Device::default();
        device.on_connected("Nothing Ear (3)".into(), "001122334455".into());
        device.values.insert(Key::Preset, Value::Byte(0));
        let (snapshot, skipped) = DeviceSnapshot::capture(&device, Options::default());
        assert!(snapshot.changes.is_empty());
        assert_eq!(skipped, ["EQ unavailable", "Transparency unavailable"]);
    }
}
