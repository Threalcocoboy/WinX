use crate::{Event, platform};
use nothingx_desktop::{
    preferences,
    winx_update::{self as update, Cache, CheckError, Options, Verified, epoch},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use tao::event_loop::EventLoopProxy;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
struct Saved {
    options: Options,
    cache: Cache,
    notified: Vec<String>,
    retry_at: u64,
}

#[derive(Debug)]
pub enum Message {
    Checked(Result<Cache, CheckError>),
    Progress(u64),
    Downloaded(Result<Verified, String>),
}

#[derive(PartialEq, Eq)]
enum Phase {
    Idle,
    Checking,
    Downloading,
    Ready,
    Restoring,
    Failed,
}

pub struct Updates {
    saved: Saved,
    phase: Phase,
    error: String,
    received: u64,
    next: Option<Instant>,
    verified: Option<Verified>,
    restore_until: Option<Instant>,
    restore_attempted: bool,
    directory: PathBuf,
}

impl Updates {
    pub fn new(now: Instant, scheduled: bool) -> Self {
        Self::load(platform::data_dir(), now, scheduled)
    }

    fn load(directory: PathBuf, now: Instant, scheduled: bool) -> Self {
        let saved: Saved = preferences::load_json(&directory.join("winx-updates.json"));
        let mut service = Self {
            saved,
            phase: Phase::Idle,
            error: String::new(),
            received: 0,
            next: None,
            verified: None,
            restore_until: None,
            restore_attempted: false,
            directory,
        };
        if scheduled {
            service.schedule(now + Duration::from_secs(2));
        }
        service
    }

    fn commit(&mut self, candidate: Saved) -> Result<(), &'static str> {
        preferences::save_json(&self.directory.join("winx-updates.json"), &candidate)
            .map_err(|_| "Could not save update preferences. Check access to your WinX profile.")?;
        self.saved = candidate;
        Ok(())
    }

    fn schedule(&mut self, now: Instant) {
        self.next = self.saved.options.automatic.then(|| {
            let checked = if self.saved.cache.prereleases == self.saved.options.prereleases
                && self.saved.cache.checked != 0
            {
                self.saved.cache.checked.saturating_add(update::DAY)
            } else {
                0
            };
            let delay = checked
                .max(self.saved.retry_at)
                .saturating_sub(epoch())
                .min(update::DAY * 7);
            now + Duration::from_secs(delay)
        });
    }

    fn release(&self) -> Option<&update::Release> {
        (self.saved.cache.prereleases == self.saved.options.prereleases)
            .then_some(self.saved.cache.release.as_ref())
            .flatten()
    }

    fn available(&self) -> bool {
        self.release()
            .is_some_and(|release| update::is_newer(&release.version, update::CURRENT))
    }

    fn busy(&self) -> bool {
        matches!(
            self.phase,
            Phase::Checking | Phase::Downloading | Phase::Restoring
        )
    }

    pub fn restoring(&self) -> bool {
        self.phase == Phase::Restoring
    }

    pub fn deadline(&self) -> Option<Instant> {
        if self.busy() || self.phase == Phase::Ready {
            self.restore_until
        } else {
            self.next
        }
    }

    pub fn tick(&mut self, now: Instant, proxy: &EventLoopProxy<Event>) {
        if self.deadline().is_some_and(|at| now >= at) && !self.restoring() {
            self.check(proxy);
        }
    }

    pub fn fail(&mut self, message: impl Into<String>) {
        self.phase = Phase::Failed;
        self.restore_until = None;
        self.error = message.into();
    }

    pub fn check(&mut self, proxy: &EventLoopProxy<Event>) {
        if self.busy() || self.phase == Phase::Ready {
            return;
        }
        if self.saved.retry_at > epoch() {
            self.fail("GitHub is limiting update checks. Try again after the cooldown.");
            return;
        }
        self.phase = Phase::Checking;
        self.next = None;
        let prereleases = self.saved.options.prereleases;
        let cached = self.saved.cache.clone();
        let proxy = proxy.clone();
        std::thread::spawn(move || {
            let _ = proxy.send_event(Event::WinXUpdate(Message::Checked(update::check(
                prereleases,
                &cached,
            ))));
        });
    }

    pub fn download(&mut self, proxy: &EventLoopProxy<Event>) {
        if self.busy() || self.phase == Phase::Ready || !self.available() {
            return;
        }
        let Some(release) = self.release().cloned() else {
            return;
        };
        self.phase = Phase::Downloading;
        self.received = 0;
        let proxy = proxy.clone();
        let directory = self.directory.clone();
        std::thread::spawn(move || {
            let result = update::download(&release, &directory, |count| {
                let _ = proxy.send_event(Event::WinXUpdate(Message::Progress(count)));
            });
            let _ = proxy.send_event(Event::WinXUpdate(Message::Downloaded(result)));
        });
    }

    pub fn message(&mut self, message: Message, now: Instant) -> Option<String> {
        match message {
            Message::Checked(result) => {
                let mut candidate = self.saved.clone();
                match result {
                    Ok(cache) => {
                        candidate.cache = cache;
                        candidate.retry_at = 0;
                        let notice = candidate
                            .cache
                            .release
                            .as_ref()
                            .filter(|release| {
                                candidate.options.notify
                                    && update::is_newer(&release.version, update::CURRENT)
                                    && !candidate.notified.contains(&release.version)
                            })
                            .map(|release| release.version.clone());
                        if let Some(version) = &notice {
                            if candidate.notified.len() >= 32 {
                                candidate.notified.remove(0);
                            }
                            candidate.notified.push(version.clone());
                        }
                        match self.commit(candidate.clone()) {
                            Ok(()) => {
                                self.phase = Phase::Idle;
                                self.schedule(now);
                                return notice;
                            }
                            Err(message) => {
                                self.saved.cache = candidate.cache;
                                self.fail(message);
                                self.schedule(now);
                            }
                        }
                    }
                    Err(error) => {
                        candidate.retry_at = error.retry_at.unwrap_or_default();
                        if self.commit(candidate).is_err() {
                            self.saved.retry_at = error.retry_at.unwrap_or_default();
                        }
                        self.fail(error.message);
                        self.next = self.saved.options.automatic.then(|| {
                            now + Duration::from_secs(
                                self.saved.retry_at.saturating_sub(epoch()).max(900),
                            )
                        });
                    }
                }
            }
            Message::Progress(count) => self.received = count,
            Message::Downloaded(Ok(verified)) => {
                self.verified = Some(verified);
                self.phase = Phase::Ready;
                self.error.clear();
            }
            Message::Downloaded(Err(message)) => self.fail(message),
        }
        None
    }

    pub fn preference(&mut self, key: &str, enabled: bool, now: Instant) {
        // A channel change while checking or downloading could authorize a stale preview.
        let mut candidate = self.saved.clone();
        match key {
            "automatic" => candidate.options.automatic = enabled,
            "notify" => candidate.options.notify = enabled,
            "prereleases" if !self.busy() && self.phase != Phase::Ready => {
                candidate.options.prereleases = enabled;
            }
            _ => return,
        }
        if let Err(message) = self.commit(candidate) {
            self.error = message.into();
        } else {
            self.error.clear();
            self.schedule(now + Duration::from_secs(2));
        }
    }

    pub fn open_release(&self) {
        let page = self
            .release()
            .and_then(|release| semver::Version::parse(&release.version).ok())
            .filter(|v| v.build.is_empty())
            .map(|version| format!("{}/tag/v{version}", update::RELEASES));
        platform::open_url(page.as_deref().unwrap_or(update::RELEASES));
    }

    pub fn dismiss_install(&mut self, now: Instant) {
        if self.phase == Phase::Ready {
            self.verified.take();
            self.phase = Phase::Idle;
            self.schedule(now + Duration::from_secs(2));
        }
    }

    pub fn confirm(&mut self, now: Instant) -> bool {
        if self.phase != Phase::Ready || self.verified.is_none() || !self.available() {
            return false;
        }
        self.phase = Phase::Restoring;
        self.restore_until = Some(now + Duration::from_secs(20));
        self.restore_attempted = false;
        true
    }

    pub fn should_retry_talk(&mut self, now: Instant, safe: bool, transitioning: bool) -> bool {
        if !self.restoring()
            || safe
            || transitioning
            || self.restore_attempted
            || self.restore_until.is_some_and(|end| now >= end)
        {
            return false;
        }
        self.restore_attempted = true;
        true
    }

    pub fn finish_restore(
        &mut self,
        now: Instant,
        safe: bool,
        transitioning: bool,
        save: impl FnOnce() -> Result<(), String>,
    ) -> bool {
        if !self.restoring() {
            return false;
        }
        if safe {
            let result = save().and_then(|()| {
                self.verified
                    .as_mut()
                    .ok_or("The verified installer is no longer available.".to_owned())?
                    .launch()
            });
            match result {
                Ok(()) => return true,
                Err(message) => self.fail(message),
            }
        } else if !transitioning || self.restore_until.is_some_and(|end| now >= end) {
            self.fail("WinX could not finish Talk Mode recovery. Reconnect the original device or output, then use Settings → Talk Mode → Retry Recovery before trying again.");
        } else {
            return false;
        }
        self.verified.take();
        self.schedule(now + Duration::from_secs(900));
        false
    }

    pub fn snapshot(&self) -> Value {
        let release = self.release();
        let available = self.available();
        let status = match self.phase {
            Phase::Idle if available => "Update available",
            Phase::Idle if release.is_some() => "You're up to date",
            Phase::Idle => "Waiting to check",
            Phase::Checking => "Checking for updates",
            Phase::Downloading => "Downloading update",
            Phase::Ready => "Ready to install",
            Phase::Restoring => "Finishing Talk Mode recovery",
            Phase::Failed => "Update failed",
        };
        json!({
            "current": update::CURRENT, "latest": release.map(|r| &r.version),
            "notes": release.map(|r| &r.notes), "status": status, "options": self.saved.options,
            "checked": (self.saved.cache.checked != 0).then_some(self.saved.cache.checked),
            "available": available, "busy": self.busy(), "ready": self.phase == Phase::Ready,
            "downloading": self.phase == Phase::Downloading, "received": self.received,
            "bytes": release.map(|r| r.size), "error": self.error,
        })
    }
}

pub fn acknowledge(args: &[String]) {
    let Some(run) = args
        .iter()
        .position(|s| s == "--update-ack")
        .and_then(|i| args.get(i + 1))
    else {
        return;
    };
    let Some((pid, stamp)) = run.split_once('-') else {
        return;
    };
    if pid.parse::<u32>().is_err() || stamp.parse::<u128>().is_err() {
        return;
    }
    let directory = platform::data_dir().join("updates").join(run);
    let _ = std::fs::write(directory.join("started"), update::CURRENT);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn directory() -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "winx-update-state-{}-{id}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn checks_persist_the_timestamp_channel_and_notification_history() {
        let dir = directory();
        let now = Instant::now();
        let mut state = Updates::load(dir.clone(), now, true);
        let version = format!(
            "{}.0.0",
            semver::Version::parse(update::CURRENT).unwrap().major + 1
        );
        let cache: Cache = serde_json::from_value(json!({
            "checked": epoch(), "prereleases": false, "release": {
                "version": version, "page": "", "notes": "Release notes", "name": "fixture",
                "installer": "", "size": 100, "checksum": "", "checksumSize": 64,
            },
        }))
        .unwrap();
        assert_eq!(
            state.message(Message::Checked(Ok(cache.clone())), now),
            Some(version.clone())
        );
        let mut reopened = Updates::load(dir.clone(), now, true);
        assert_eq!(reopened.snapshot()["latest"], version);
        assert!(reopened.deadline().unwrap() > now + Duration::from_secs(update::DAY / 2));
        assert_eq!(reopened.message(Message::Checked(Ok(cache)), now), None);
        reopened.preference("automatic", false, now);
        assert!(reopened.deadline().is_none());
        reopened.preference("prereleases", true, now);
        assert!(reopened.snapshot()["latest"].is_null());
        std::fs::remove_file(dir.join("winx-updates.json")).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn update_waits_for_talk_mode_and_aborts_if_recovery_stays_pending() {
        let dir = directory();
        let now = Instant::now();
        let mut state = Updates::load(dir.clone(), now, false);
        assert!(!state.confirm(now));
        state.phase = Phase::Restoring;
        state.restore_until = Some(now + Duration::from_secs(20));
        assert!(!state.should_retry_talk(now, false, true));
        assert!(!state.finish_restore(now, false, true, || panic!("must not save or install yet")));
        assert!(state.should_retry_talk(now, false, false));
        assert!(!state.should_retry_talk(now, false, false));
        assert!(!state.finish_restore(now, false, false, || panic!(
            "must not save or install with recovery pending"
        )));
        assert_eq!(state.snapshot()["status"], "Update failed");
        assert!(state.error.contains("Talk Mode"));
        std::fs::remove_dir(dir).unwrap();
    }
}
