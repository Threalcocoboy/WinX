//! Serialized Talk Mode transitions on the same thread as the Bluetooth Device.
use crate::{
    Event,
    talk_audio_windows::{self as audio, Job},
};
use nothingx_desktop::{
    awareness::NoticeAction,
    device::{Action, Device, Key},
    preferences,
    talk::{DeviceSnapshot, Options, Restore},
};
use serde_json::{Value as Json, json};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tao::event_loop::EventLoopProxy;

const DEVICE_JOURNAL: &str = "winx-talk-device.json";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Off,
    Starting,
    Active,
    Stopping,
    Recovery,
}

pub struct TalkMode {
    phase: Phase,
    options: Options,
    snapshot: DeviceSnapshot,
    device_step: usize,
    waiting: Option<Instant>,
    restore_read: Option<(u64, Instant)>,
    restore_until: Instant,
    automatic_recovery: bool,
    retry_parked: bool,
    audio_deadline: Instant,
    fade_started: bool,
    last_connected: String,
    dispatch_audio: Box<dyn Fn(u64, Duration, Job)>,
    directory: PathBuf,
    job: Option<(u64, bool)>,
    generation: u64,
    settled_audio: bool,
    captured_audio: bool,
    volume: Option<u8>,
    cancel: Arc<AtomicBool>,
    notes: Vec<&'static str>,
    detail: String,
    feedback: Option<(NoticeAction, String, String)>,
    transparency: bool,
    eq: bool,
    failed_activation: bool,
}

impl TalkMode {
    pub fn new(directory: PathBuf, proxy: EventLoopProxy<Event>, now: Instant) -> Self {
        let audio_directory = directory.clone();
        Self::with_audio(directory, now, move |generation, duration, job| {
            audio::start(
                proxy.clone(),
                audio_directory.clone(),
                generation,
                duration,
                job,
            );
        })
    }

    fn with_audio(
        directory: PathBuf,
        now: Instant,
        dispatch_audio: impl Fn(u64, Duration, Job) + 'static,
    ) -> Self {
        let snapshot: DeviceSnapshot = preferences::load_json(&directory.join(DEVICE_JOURNAL));
        let phase = if audio::pending_outputs(&directory) != 0
            || snapshot.changes.iter().any(|item| item.attempted)
        {
            Phase::Recovery
        } else {
            Phase::Off
        };
        let mut talk = Self {
            phase,
            options: Options::default(),
            directory,
            dispatch_audio: Box::new(dispatch_audio),
            snapshot,
            device_step: 0,
            waiting: None,
            restore_read: None,
            restore_until: now,
            automatic_recovery: phase == Phase::Recovery,
            retry_parked: false,
            audio_deadline: now,
            fade_started: false,
            last_connected: String::new(),
            job: None,
            generation: 0,
            volume: None,
            settled_audio: false,
            captured_audio: false,
            transparency: false,
            eq: false,
            cancel: Arc::new(AtomicBool::new(false)),
            notes: Vec::new(),
            detail: String::new(),
            feedback: None,
            failed_activation: false,
        };
        if talk.earbud_records() > 0 {
            talk.phase = Phase::Recovery;
            talk.automatic_recovery = true;
        }
        talk
    }

    pub fn status(&self) -> Json {
        json!({
            "phase": match self.phase {
                Phase::Off => "off", Phase::Starting => "activating", Phase::Active => "active",
                Phase::Stopping => "restoring", Phase::Recovery => "recovery",
            },
            "detail": self.detail, "volume": self.volume, "transparency": self.transparency, "eq": self.eq,
            "complete": self.notes.is_empty() && !self.failed_activation,
            "outputRecords": audio::pending_outputs(&self.directory),
            "earbudRecords": self.earbud_records(),
            "canArchive": !self.transitioning() && self.phase != Phase::Active && self.job.is_none(),
        })
    }

    pub fn toggle(&mut self, device: &mut Device, verified: bool, options: Options, now: Instant) {
        match self.phase {
            Phase::Stopping => return,
            Phase::Starting | Phase::Active => {
                self.stop(now, false);
                return;
            }
            Phase::Recovery | Phase::Off => {}
        }
        if !options.enabled
            || !verified
            || [Key::Noise, Key::Preset, Key::CustomEq]
                .iter()
                .any(|key| device.is_pending(*key))
        {
            self.options = options;
            self.notes = vec![if !options.enabled {
                "Enable Talk Mode in Settings"
            } else if !verified {
                "Connect and select compatible earbuds"
            } else {
                "Wait for the current earbud setting to finish"
            }];
            self.report(
                NoticeAction::TalkPartial,
                "Talk Mode unavailable",
                self.notes[0],
            );
            return;
        }
        if self.load_device_recovery(&device.address) {
            self.retry(device, now);
            return;
        }
        self.options = options;
        (self.snapshot, self.notes) = DeviceSnapshot::capture(device, options);
        self.phase = Phase::Starting;
        self.waiting = None;
        self.device_step = 0;
        self.volume = None;
        self.transparency = false;
        self.eq = false;
        self.failed_activation = false;
        self.settled_audio = false;
        self.captured_audio = false;
        self.fade_started = false;
        self.detail = "Applying your conversation settings…".into();
        self.cancel = Arc::new(AtomicBool::new(false));
        self.start_audio(
            now,
            Job::Prepare {
                address: device.address.clone(),
            },
        );
    }

    fn start_audio(&mut self, now: Instant, job: Job) {
        if self.job.is_some() {
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        self.job = Some((self.generation, matches!(&job, Job::Restore { .. })));
        self.audio_deadline = now + Duration::from_secs(5);
        (self.dispatch_audio)(
            self.generation,
            Duration::from_millis(u64::from(self.options.fade_ms)),
            job,
        );
    }

    pub fn audio_result(&mut self, generation: u64, result: audio::Result, now: Instant) {
        let Some((expected, restoring)) = self.job else {
            return;
        };
        if generation != expected {
            return;
        }
        self.job = None;
        if restoring {
            self.settled_audio = true;
            self.volume = None;
            match result {
                audio::Result::Preserved => self.notes.push("Your newer volume was kept"),
                audio::Result::Unchanged | audio::Result::Restored => {}
                _ => self.notes.push("Original output is awaiting recovery"),
            }
            return;
        }
        if self.phase != Phase::Starting {
            return;
        }
        self.captured_audio = true;
        self.settled_audio = result != audio::Result::Prepared;
        match result {
            audio::Result::Prepared => {}
            audio::Result::Applied(volume) => {
                self.volume = Some(volume);
                if volume != self.options.volume {
                    self.notes
                        .push("The output could not reach the exact target");
                }
            }
            audio::Result::PendingOutput => self
                .notes
                .push("Volume left unchanged because its recovery record is unresolved"),
            audio::Result::Unavailable => self.notes.push("No verified earbud audio output"),
            audio::Result::Preserved => self.notes.push("Your newer volume was kept"),
            _ => self.fail("Windows could not apply the volume fade", now),
        }
    }

    pub fn stop(&mut self, now: Instant, link_lost: bool) {
        if self.phase == Phase::Off || self.phase == Phase::Stopping {
            return;
        }
        self.phase = Phase::Stopping;
        self.cancel.store(true, Ordering::Release);
        self.waiting = None;
        // Reverse traversal restores EQ filters/preset first and noise mode second.
        self.snapshot.changes.sort_by_key(|item| match item.key {
            Key::Noise => 0,
            Key::Preset => 1,
            _ => 2,
        });
        self.device_step = self.snapshot.changes.len();
        self.restore_read = None;
        self.settled_audio = false;
        self.restore_until = now + Duration::from_secs(9);
        self.detail = "Restoring your listening settings…".into();
        self.notes.clear();
        if link_lost && self.snapshot.changes.iter().any(|item| item.attempted) {
            self.notes
                .push("Earbud recovery will resume on reconnection");
        }
    }

    fn fail(&mut self, reason: &'static str, now: Instant) {
        self.failed_activation = true;
        self.stop(now, false);
        self.notes.push(reason);
    }

    pub fn tick(&mut self, device: &mut Device, verified: bool, now: Instant) {
        if let Some((generation, restoring)) = self.job
            && now >= self.audio_deadline
        {
            self.cancel.store(true, Ordering::Release);
            self.audio_result(
                generation,
                if restoring {
                    audio::Result::Deferred
                } else {
                    audio::Result::Failed
                },
                now,
            );
        }
        let previous_address = std::mem::take(&mut self.last_connected);
        if verified {
            self.last_connected.clone_from(&device.address);
        }
        let same_device = verified && device.address == self.snapshot.address;
        match self.phase {
            Phase::Off => {}
            Phase::Recovery => {
                if self.automatic_recovery {
                    self.automatic_recovery = false;
                    self.retry(device, now);
                } else if verified
                    && device.address != previous_address
                    && (self.load_device_recovery(&device.address)
                        || audio::pending_outputs(&self.directory) > 0)
                {
                    self.retry(device, now);
                }
            }
            Phase::Starting if !same_device => {
                self.fail("The earbuds disconnected during activation", now)
            }
            Phase::Active if !same_device => self.stop(now, true),
            Phase::Starting => self.advance_activation(device, now),
            Phase::Stopping => self.advance_restore(device, same_device, now),
            Phase::Active => {}
        }
    }

    fn advance_activation(&mut self, device: &mut Device, now: Instant) {
        if !self.captured_audio {
            return;
        }
        if !self.fade_started {
            self.fade_started = true;
            if !self.settled_audio {
                self.start_audio(
                    now,
                    Job::Apply {
                        target: self.options.volume,
                        cancel: self.cancel.clone(),
                    },
                );
            }
        }
        let next_is_eq = self
            .snapshot
            .changes
            .get(self.device_step)
            .is_some_and(|change| change.key != Key::Noise);
        if !next_is_eq && !self.settled_audio {
            return;
        }
        let Some(change) = self.snapshot.changes.get_mut(self.device_step) else {
            self.transparency =
                self.options.transparency && !self.notes.contains(&"Transparency unavailable");
            self.eq = self.options.reduce_eq && !self.notes.contains(&"EQ unavailable");
            self.phase = Phase::Active;
            let mut actual = Vec::new();
            if self.transparency {
                actual.push("Transparency".to_string());
            }
            if let Some(volume) = self.volume {
                actual.push(format!("Volume {volume}%"));
            }
            if self.eq && (actual.len() < 2 || !self.notes.is_empty()) {
                actual.push("Speech EQ".into());
            }
            if actual.is_empty() && !self.notes.is_empty() {
                self.phase = Phase::Off;
                self.report(
                    NoticeAction::TalkPartial,
                    "Talk Mode unavailable",
                    &self.notes.join(" · "),
                );
                return;
            }
            if !self.notes.is_empty() {
                actual.push(self.notes.join(" · "));
            }
            self.report(
                if self.notes.is_empty() {
                    NoticeAction::TalkOn
                } else {
                    NoticeAction::TalkPartial
                },
                "Talk Mode",
                &actual.join(" · "),
            );
            return;
        };
        if let Some(deadline) = self.waiting {
            if !device.is_pending(change.key) && device.value(change.key) == Some(&change.target) {
                self.device_step += 1;
                self.waiting = None;
            } else if now >= deadline {
                self.fail("The earbuds did not confirm a Talk Mode setting", now);
            }
            return;
        }
        if change.restore(device) != Restore::Original {
            self.fail(
                "An earbud setting changed while Talk Mode was starting",
                now,
            );
            return;
        }
        let command = Action::Set(change.key, change.target.clone());
        change.attempted = true;
        if !self.save_device() {
            self.snapshot.changes[self.device_step].attempted = false;
            self.fail("WinX could not save the earbud recovery state", now);
            return;
        }
        device.action(command, now);
        self.waiting = Some(now + Duration::from_millis(5_500));
    }

    fn advance_restore(&mut self, device: &mut Device, same_device: bool, now: Instant) {
        if self.device_step > 0 && same_device {
            self.restore_device_step(device, now);
        }
        if self.device_step > 0 && same_device && now < self.restore_until {
            return;
        }
        if !self.settled_audio {
            if self.job.is_none() {
                let retry_parked = std::mem::take(&mut self.retry_parked);
                self.start_audio(now, Job::Restore { retry_parked });
            }
            return;
        }
        let device_left = self.snapshot.changes.iter().any(|item| item.attempted);
        if device_left
            && !self
                .notes
                .contains(&"Earbud recovery will resume on reconnection")
        {
            self.notes.push("Earbud settings are awaiting recovery");
        }
        self.phase = if device_left
            || self.earbud_records() != 0
            || audio::pending_outputs(&self.directory) != 0
        {
            Phase::Recovery
        } else {
            Phase::Off
        };

        self.transparency = false;
        self.eq = false;
        if self.phase == Phase::Recovery && self.notes.is_empty() {
            self.notes
                .push("Saved recovery is pending for an unavailable target. Retry in Settings.");
        }
        if self.notes.is_empty() {
            self.report(
                NoticeAction::TalkOff,
                "Talk Mode Off",
                "Original settings restored",
            );
        } else {
            self.report(
                NoticeAction::TalkPartial,
                "Talk Mode recovery",
                &self.notes.join(" · "),
            );
        }
    }

    fn restore_device_step(&mut self, device: &mut Device, now: Instant) {
        let item = &mut self.snapshot.changes[self.device_step - 1];
        if item.attempted && self.waiting.is_none() {
            if device.is_pending(item.key) {
                return;
            }
            let revision = device.revision(item.key);
            match self.restore_read {
                None => {
                    device.request_setting(item.key);
                    self.restore_read = Some((revision, now + Duration::from_secs(3)));
                    return;
                }
                Some((read, deadline)) if read == revision => {
                    if now >= deadline {
                        self.device_step -= 1;
                        self.restore_read = None;
                    }
                    return;
                }
                _ => {}
            }
        }
        let observed = item.restore(device);
        if !item.attempted || observed == Restore::Original {
            item.attempted = false;
            self.device_step -= 1;
            self.waiting = None;
            self.restore_read = None;
            if !self.save_device() {
                self.notes
                    .push("WinX could not clear the earbud recovery state");
            }
        } else if observed == Restore::Changed {
            let eq = matches!(item.key, Key::Preset | Key::CustomEq);
            for item in &mut self.snapshot.changes {
                if !eq && item.key == Key::Noise
                    || eq && matches!(item.key, Key::Preset | Key::CustomEq)
                {
                    item.attempted = false;
                }
            }
            self.notes.push("Your newer earbud settings were kept");
            self.waiting = None;
            self.restore_read = None;
            let _ = self.save_device();
        } else if observed == Restore::Owned && self.waiting.is_none() {
            device.action(Action::Set(item.key, item.original.clone()), now);
            self.waiting = Some(now + Duration::from_millis(5_500));
        } else if self.waiting.is_some_and(|deadline| now >= deadline) {
            self.device_step -= 1;
            self.waiting = None;
            self.restore_read = None;
        }
    }

    fn device_path(&self, address: &str) -> Option<PathBuf> {
        (address.len() == 12 && address.bytes().all(|byte| byte.is_ascii_hexdigit())).then(|| {
            self.directory
                .join(format!("winx-talk-buds-{address}.json"))
        })
    }

    fn earbud_paths(&self) -> Vec<PathBuf> {
        std::fs::read_dir(&self.directory)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name == DEVICE_JOURNAL
                            || (name.starts_with("winx-talk-buds-") && name.ends_with(".json"))
                    })
            })
            .collect()
    }

    fn earbud_records(&self) -> usize {
        self.earbud_paths().len()
    }

    fn load_device_recovery(&mut self, address: &str) -> bool {
        let legacy = self.directory.join(DEVICE_JOURNAL);
        if legacy.exists() {
            let record: DeviceSnapshot = preferences::load_json(&legacy);
            let Some(parked) = self.device_path(&record.address) else {
                return true;
            };
            if !parked.exists() && std::fs::rename(&legacy, parked).is_err() {
                return true;
            }
            if legacy.exists() {
                return true;
            }
        }
        let Some(path) = self.device_path(address).filter(|path| path.exists()) else {
            return false;
        };
        let record: DeviceSnapshot = preferences::load_json(&path);
        if record.address == address {
            self.snapshot = record;
        }
        true
    }

    pub fn retry(&mut self, device: &Device, now: Instant) {
        if self.transitioning() {
            return;
        }
        if self.phase != Phase::Active {
            self.load_device_recovery(&device.address);
            self.retry_parked = true;
            self.phase = Phase::Recovery;
        }
        self.stop(now, false);
    }

    pub fn archive(&mut self, now: Instant) -> std::io::Result<()> {
        if self.transitioning() || self.phase == Phase::Active || self.job.is_some() {
            return Err(std::io::Error::other(
                "Finish the current transition first.",
            ));
        }
        audio::archive_outputs(&self.directory)?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for path in self.earbud_paths() {
            std::fs::rename(&path, path.with_extension(format!("{stamp}.archived")))?;
        }
        self.snapshot = DeviceSnapshot::default();
        self.phase = Phase::Off;
        self.automatic_recovery = false;
        self.notes.clear();
        self.detail = "Recovery records archived. Current settings were kept.".into();
        self.retry_parked = false;
        self.restore_until = now;
        Ok(())
    }

    fn save_device(&self) -> bool {
        let Some(path) = self.device_path(&self.snapshot.address) else {
            return false;
        };
        if self.snapshot.changes.iter().any(|item| item.attempted) {
            preferences::save_json(&path, &self.snapshot).is_ok()
        } else {
            std::fs::remove_file(&path).is_ok() || !path.exists()
        }
    }

    fn report(&mut self, action: NoticeAction, title: &str, detail: &str) {
        self.detail = detail.into();
        if self.options.overlay {
            self.feedback = Some((action, title.into(), detail.into()));
        }
    }

    pub fn owns_pending(&self, key: Key) -> bool {
        if self.waiting.is_none() {
            return false;
        }
        let index = match self.phase {
            Phase::Starting => self.device_step,
            Phase::Stopping => match self.device_step.checked_sub(1) {
                Some(index) => index,
                None => return false,
            },
            _ => return false,
        };
        self.snapshot
            .changes
            .get(index)
            .is_some_and(|change| change.attempted && change.key == key)
    }

    pub fn feedback(&mut self) -> Option<(NoticeAction, String, String)> {
        self.feedback.take()
    }
    pub fn inactive(&self) -> bool {
        matches!(self.phase, Phase::Off | Phase::Recovery)
    }
    pub fn transitioning(&self) -> bool {
        matches!(self.phase, Phase::Starting | Phase::Stopping)
    }
    pub fn safe_to_update(&self) -> bool {
        self.phase == Phase::Off
            && self.job.is_none()
            && self.earbud_records() == 0
            && audio::pending_outputs(&self.directory) == 0
    }
    pub fn deadline(&self, now: Instant) -> Option<Instant> {
        if self.transitioning() {
            Some(now + Duration::from_millis(65))
        } else if self.automatic_recovery {
            Some(now)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nothingx_desktop::{device::Value, protocol};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn scratch() -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "winx-talk-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&root).unwrap();
        root
    }

    fn report_anc(device: &mut Device, mode: u8) {
        device.receive(&protocol::encode(0x401e, &[1, mode, 0], 1).unwrap());
    }

    fn report_setting(device: &mut Device, command: u16, payload: &[u8]) {
        device.receive(&protocol::encode(command, payload, 1).unwrap());
    }

    #[test]
    fn fade_and_eq_start_together_before_transparency_and_restore_in_reverse() {
        let at = Instant::now();
        let directory = scratch();
        let mut mode = TalkMode::with_audio(directory.clone(), at, |_, _, _| {});
        let mut device = Device::default();
        device.on_connected("Nothing Ear (3)".into(), "123456789ABC".into());
        report_anc(&mut device, 3);
        let eq = &device.caps.as_ref().unwrap().eq;
        let hz = eq.treble_hz;
        let custom = eq.custom_id.unwrap();
        let old = nothingx_desktop::device::encode_eq([2, 0, -1], hz);
        let speech = nothingx_desktop::device::encode_eq([-6, 6, -6], hz);
        report_setting(&mut device, 0x4044, &old);
        device.values.insert(Key::Preset, Value::Byte(custom));
        mode.toggle(&mut device, true, Options::default(), at);
        mode.tick(&mut device, true, at);
        assert!(!device.is_pending(Key::CustomEq));
        mode.audio_result(mode.generation, audio::Result::Prepared, at);
        mode.tick(&mut device, true, at);
        assert!(mode.fade_started && mode.job.is_some());
        assert_eq!(
            device.pending_value(Key::CustomEq),
            Some(&Value::Eq([-6, 6, -6]))
        );
        assert!(!device.is_pending(Key::Noise));
        report_setting(&mut device, 0x4044, &speech);
        mode.tick(&mut device, true, at);
        mode.tick(&mut device, true, at);
        assert!(!device.is_pending(Key::Noise));
        mode.audio_result(mode.generation, audio::Result::Applied(5), at);
        mode.tick(&mut device, true, at);
        assert_eq!(device.pending_value(Key::Noise), Some(&Value::Byte(7)));
        report_anc(&mut device, 7);
        mode.tick(&mut device, true, at);
        mode.tick(&mut device, true, at);
        assert_eq!(mode.status()["phase"], "active");

        mode.toggle(&mut device, true, Options::default(), at);
        mode.tick(&mut device, true, at);
        report_setting(&mut device, 0x4044, &speech);
        mode.tick(&mut device, true, at);
        assert_eq!(
            device.pending_value(Key::CustomEq),
            Some(&Value::Eq([2, 0, -1]))
        );
        assert!(!device.is_pending(Key::Noise));
        assert!(mode.job.is_none());
        report_setting(&mut device, 0x4044, &old);
        mode.tick(&mut device, true, at);
        mode.tick(&mut device, true, at);
        report_anc(&mut device, 7);
        mode.tick(&mut device, true, at);
        assert_eq!(device.pending_value(Key::Noise), Some(&Value::Byte(3)));
        report_anc(&mut device, 3);
        mode.tick(&mut device, true, at);
        mode.audio_result(mode.generation, audio::Result::Restored, at);
        mode.tick(&mut device, true, at);
        assert_eq!(mode.status()["detail"], "Original settings restored");
        assert!(mode.earbud_paths().is_empty());
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn missing_original_output_does_not_block_a_new_session() {
        let at = Instant::now();
        let directory = scratch();
        let parked = directory.join("winx-talk-output-0.json");
        std::fs::write(
            &parked,
            br#"{"id":"missing","original":0.64,"last":0.05,"next":0.05}"#,
        )
        .unwrap();
        let mut mode = TalkMode::with_audio(directory.clone(), at, |_, _, _| {});
        let mut device = Device::default();
        mode.tick(&mut device, false, at);
        mode.tick(&mut device, false, at);
        mode.audio_result(mode.generation, audio::Result::Deferred, at);
        mode.tick(&mut device, false, at);
        assert_eq!(mode.status()["phase"], "recovery");
        let generation = mode.generation;
        mode.tick(&mut device, false, at + Duration::from_secs(60));
        assert_eq!(mode.generation, generation);

        device.on_connected("Nothing Ear (3)".into(), "123456789ABD".into());
        report_anc(&mut device, 3);
        mode.toggle(
            &mut device,
            true,
            Options {
                reduce_eq: false,
                ..Options::default()
            },
            at,
        );
        assert_eq!(mode.status()["phase"], "activating");
        assert!(parked.exists());
        mode.toggle(&mut device, true, Options::default(), at);
        assert_eq!(mode.status()["phase"], "restoring");
        mode.tick(&mut device, true, at + Duration::from_secs(6));
        mode.tick(&mut device, true, at + Duration::from_secs(12));
        assert_eq!(mode.status()["phase"], "recovery");
        assert!(parked.exists());
        mode.archive(at).unwrap();
        assert!(mode.inactive() && !parked.exists());
        let backup = std::fs::read_dir(&directory)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(backup.extension().unwrap(), "archived");
        std::fs::remove_file(backup).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn disconnected_earbud_record_can_only_be_loaded_for_its_address() {
        let at = Instant::now();
        let directory = scratch();
        let mut mode = TalkMode::with_audio(directory.clone(), at, |_, _, _| {});
        let mut device = Device::default();
        device.on_connected("Nothing Ear (3)".into(), "123456789ABC".into());
        report_anc(&mut device, 3);
        let (mut snapshot, _) = DeviceSnapshot::capture(
            &device,
            Options {
                reduce_eq: false,
                ..Options::default()
            },
        );
        snapshot.changes[0].attempted = true;
        preferences::save_json(&directory.join(DEVICE_JOURNAL), &snapshot).unwrap();
        assert!(!mode.load_device_recovery("123456789ABD"));
        assert!(mode.load_device_recovery("123456789ABC"));
        assert_eq!(mode.snapshot.address, "123456789ABC");
        assert!(mode.snapshot.changes[0].attempted);
        mode.archive(at).unwrap();
        let backup = std::fs::read_dir(&directory)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        std::fs::remove_file(backup).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
