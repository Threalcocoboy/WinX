//! Correlates an identified consumer-control command with a corresponding
//! outcome. A Windows playback or volume sample cannot arm this tracker.
use crate::awareness::NoticeAction;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Play,
    Pause,
    TogglePlayback,
    Next,
    Previous,
    Louder,
    Quieter,
    ToggleMute,
}

impl Command {
    pub fn from_consumer_usage(usage: u16) -> Option<Self> {
        Some(match usage {
            0xb0 => Self::Play,
            0xb1 => Self::Pause,
            0xcd => Self::TogglePlayback,
            0xb5 => Self::Next,
            0xb6 => Self::Previous,
            0xe9 => Self::Louder,
            0xea => Self::Quieter,
            0xe2 => Self::ToggleMute,
            _ => return None,
        })
    }

    pub fn enabled(self, options: crate::preferences::MediaOptions) -> bool {
        match self {
            Self::Play | Self::Pause | Self::TogglePlayback => options.play_pause,
            Self::Next | Self::Previous => options.track,
            _ => options.volume,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HardwareSource {
    PairedHeadset,
    RemovableUsb,
}

#[derive(Clone, Copy, Debug)]
pub struct HardwareCommand {
    pub action: Command,
    pub source: HardwareSource,
    pub sequence: u64,
    pub at: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Playback {
    Playing,
    Paused,
    Stopped,
    Other,
}

#[derive(Clone, Debug)]
pub struct Session {
    pub identity: usize,
    pub app: String,
    pub playback: Playback,
    pub can_next: bool,
    pub can_previous: bool,
    pub track: Option<Track>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub number: i32,
}

#[derive(Clone, Debug)]
pub struct Output {
    pub id: String,
    pub scalar: f32,
    pub muted: bool,
}

#[derive(Clone, Debug)]
pub struct Sample {
    // Capture is an interval: a read that straddles the command cannot be
    // used as either the pre-command baseline or the confirmed outcome.
    pub started: Instant,
    pub finished: Instant,
    pub session: Option<Session>,
    pub output: Option<Output>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Confirmation {
    pub action: NoticeAction,
    pub detail: String,
}

const BASELINE_AGE: Duration = Duration::from_millis(650);
const CONFIRMATION_LIMIT: Duration = Duration::from_millis(1100);

struct Pending {
    event: HardwareCommand,
    baseline: Sample,
}

#[derive(Default)]
pub struct Correlator {
    samples: VecDeque<Sample>,
    pending: Option<Pending>,
    sequence: u64,
}

impl Correlator {
    pub fn armed(&self) -> bool {
        self.pending.is_some()
    }

    pub fn arm(&mut self, event: HardwareCommand, now: Instant) {
        if event.sequence <= self.sequence
            || now.saturating_duration_since(event.at) > CONFIRMATION_LIMIT
        {
            return;
        }
        self.sequence = event.sequence;
        // Do not reuse an earlier pending command when the event stream gets
        // ambiguous. The most recent command needs its own fresh baseline.
        self.pending = self
            .samples
            .iter()
            .rev()
            .find(|s| s.finished < event.at && event.at.duration_since(s.finished) <= BASELINE_AGE)
            .filter(|sample| valid_baseline(sample, event.action))
            .map(|baseline| Pending {
                event,
                baseline: baseline.clone(),
            });
    }

    pub fn observe(&mut self, sample: Sample) -> Option<Confirmation> {
        let result = self.pending.take().and_then(|pending| {
            if sample.finished.saturating_duration_since(pending.event.at) > CONFIRMATION_LIMIT {
                return None;
            }
            if sample.started <= pending.event.at {
                self.pending = Some(pending);
                return None;
            }
            match outcome(&pending.baseline, &sample, pending.event.action) {
                Outcome::Waiting => {
                    self.pending = Some(pending);
                    None
                }
                Outcome::Conflict => None,
                Outcome::Confirmed(value) => Some(value),
            }
        });
        if self.samples.len() == 8 {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
        result
    }
}

fn valid_baseline(sample: &Sample, action: Command) -> bool {
    match action {
        Command::Louder | Command::Quieter | Command::ToggleMute => sample.output.is_some(),
        _ => sample.session.as_ref().is_some_and(|session| match action {
            Command::Play => matches!(session.playback, Playback::Paused | Playback::Stopped),
            Command::Pause => session.playback == Playback::Playing,
            Command::TogglePlayback => {
                matches!(
                    session.playback,
                    Playback::Playing | Playback::Paused | Playback::Stopped
                )
            }
            Command::Next => session.can_next && session.track.is_some(),
            Command::Previous => session.can_previous && session.track.is_some(),
            _ => false,
        }),
    }
}

enum Outcome {
    Waiting,
    Conflict,
    Confirmed(Confirmation),
}

fn outcome(before: &Sample, after: &Sample, action: Command) -> Outcome {
    let confirm = |action, detail| Outcome::Confirmed(Confirmation { action, detail });
    if matches!(
        action,
        Command::Louder | Command::Quieter | Command::ToggleMute
    ) {
        let (Some(old), Some(new)) = (&before.output, &after.output) else {
            return Outcome::Conflict;
        };
        if old.id != new.id {
            return Outcome::Conflict;
        }
        let direction = new.scalar - old.scalar;
        let level = format!("{}%", (new.scalar * 100.).round() as u8);
        return match action {
            Command::ToggleMute if new.muted != old.muted && direction.abs() < 0.001 => confirm(
                if new.muted {
                    NoticeAction::Mute
                } else {
                    NoticeAction::Unmute
                },
                level,
            ),
            Command::Louder if direction >= 0.001 && !new.muted => {
                confirm(NoticeAction::VolumeUp, level)
            }
            Command::Quieter if direction <= -0.001 && !new.muted => {
                confirm(NoticeAction::VolumeDown, level)
            }
            _ if direction.abs() >= 0.001 || new.muted != old.muted => Outcome::Conflict,
            _ => Outcome::Waiting,
        };
    }
    let (Some(old), Some(new)) = (&before.session, &after.session) else {
        return Outcome::Conflict;
    };
    if old.identity != new.identity || old.app != new.app {
        return Outcome::Conflict;
    }
    if matches!(action, Command::Next | Command::Previous) {
        return match (&old.track, &new.track) {
            (Some(old), Some(new))
                if old.title != new.title
                    || (old.number > 0 && new.number > 0 && old.number != new.number) =>
            {
                confirm(
                    if action == Command::Next {
                        NoticeAction::NextTrack
                    } else {
                        NoticeAction::PreviousTrack
                    },
                    new.title.clone(),
                )
            }
            (_, None) => Outcome::Waiting,
            _ => Outcome::Waiting,
        };
    }
    if new.playback == old.playback {
        return Outcome::Waiting;
    }
    if old
        .track
        .as_ref()
        .zip(new.track.as_ref())
        .is_some_and(|(old, new)| old.title != new.title)
    {
        return Outcome::Conflict;
    }
    let action = match (action, old.playback, new.playback) {
        (
            Command::Play | Command::TogglePlayback,
            Playback::Paused | Playback::Stopped,
            Playback::Playing,
        ) => NoticeAction::Play,
        (Command::Pause | Command::TogglePlayback, Playback::Playing, Playback::Paused) => {
            NoticeAction::Pause
        }
        _ => return Outcome::Conflict,
    };
    confirm(action, String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(t: Instant, playing: bool, track: &str, level: f32, muted: bool) -> Sample {
        Sample {
            started: t,
            finished: t + Duration::from_millis(1),
            session: Some(Session {
                identity: 1,
                app: "Player".into(),
                playback: if playing {
                    Playback::Playing
                } else {
                    Playback::Paused
                },
                can_next: true,
                can_previous: true,
                track: Some(Track {
                    title: track.into(),
                    artist: String::new(),
                    album: String::new(),
                    number: 0,
                }),
            }),
            output: Some(Output {
                id: "Endpoint".into(),
                scalar: level,
                muted,
            }),
        }
    }

    fn command(t: Instant, action: Command, sequence: u64) -> HardwareCommand {
        HardwareCommand {
            action,
            sequence,
            at: t,
            source: HardwareSource::PairedHeadset,
        }
    }

    #[test]
    fn genuine_hid_play_pause_skip_volume_and_mute_require_corresponding_outcomes() {
        let t = Instant::now();
        let action_time = t + Duration::from_millis(50);
        let after = t + Duration::from_millis(100);
        let mut stopped = state(t, false, "A", 0.5, false);
        stopped.session.as_mut().unwrap().playback = Playback::Stopped;
        for (action, expected, initial, final_state) in [
            (
                Command::Play,
                NoticeAction::Play,
                stopped,
                state(after, true, "A", 0.5, false),
            ),
            (
                Command::Play,
                NoticeAction::Play,
                state(t, false, "A", 0.5, false),
                state(after, true, "A", 0.5, false),
            ),
            (
                Command::Pause,
                NoticeAction::Pause,
                state(t, true, "A", 0.5, false),
                state(after, false, "A", 0.5, false),
            ),
            (
                Command::Next,
                NoticeAction::NextTrack,
                state(t, true, "A", 0.5, false),
                state(after, true, "B", 0.5, false),
            ),
            (
                Command::Previous,
                NoticeAction::PreviousTrack,
                state(t, true, "B", 0.5, false),
                state(after, true, "A", 0.5, false),
            ),
            (
                Command::Louder,
                NoticeAction::VolumeUp,
                state(t, true, "A", 0.5, false),
                state(after, true, "A", 0.6, false),
            ),
            (
                Command::Quieter,
                NoticeAction::VolumeDown,
                state(t, true, "A", 0.6, false),
                state(after, true, "A", 0.5, false),
            ),
            (
                Command::ToggleMute,
                NoticeAction::Mute,
                state(t, true, "A", 0.5, false),
                state(after, true, "A", 0.5, true),
            ),
            (
                Command::ToggleMute,
                NoticeAction::Unmute,
                state(t, true, "A", 0.5, true),
                state(after, true, "A", 0.5, false),
            ),
        ] {
            let mut correlator = Correlator::default();
            assert!(correlator.observe(initial).is_none());
            let event = command(action_time, action, 1);
            correlator.arm(event, action_time);
            assert_eq!(
                correlator.observe(final_state.clone()).unwrap().action,
                expected
            );
            correlator.arm(event, after);
            assert!(correlator.observe(final_state).is_none());
        }
    }

    #[test]
    fn startup_and_software_only_playback_track_or_slider_changes_never_arm() {
        let now = Instant::now();
        let mut correlator = Correlator::default();
        for (i, (play, track, volume)) in [
            (false, "A", 0.5),
            (true, "A", 0.5),
            (false, "A", 0.5),
            (true, "Advertisement", 0.5),
            (true, "B", 0.5),
            (true, "B", 0.65),
        ]
        .into_iter()
        .enumerate()
        {
            assert!(
                correlator
                    .observe(state(
                        now + Duration::from_millis(i as u64 * 50),
                        play,
                        track,
                        volume,
                        false
                    ))
                    .is_none()
            );
        }
        assert!(!correlator.armed());
    }

    #[test]
    fn unstable_identity_wrong_direction_disconnect_and_late_outcome_are_suppressed() {
        let now = Instant::now();
        let at = now + Duration::from_millis(55);
        let after = now + Duration::from_millis(150);
        let mut tracker = Correlator::default();
        tracker.observe(state(now, true, "A", 0.5, false));
        tracker.arm(command(at, Command::Pause, 1), at);
        let mut different = state(after, false, "A", 0.5, false);
        different.session.as_mut().unwrap().identity = 2; // same app, different session
        assert!(tracker.observe(different).is_none());
        assert!(!tracker.armed());
        tracker.arm(
            command(after + Duration::from_millis(10), Command::Louder, 2),
            after,
        );
        assert!(
            tracker
                .observe(state(
                    after + Duration::from_millis(50),
                    false,
                    "A",
                    0.4,
                    false
                ))
                .is_none()
        );
        assert!(!tracker.armed());

        tracker = Correlator::default(); // worker discarded on disconnect/resume
        tracker.arm(command(at, Command::Pause, 3), at);
        assert!(
            tracker
                .observe(state(after, false, "A", 0.5, false))
                .is_none()
        );
        tracker.arm(
            command(after + Duration::from_millis(10), Command::Play, 4),
            after,
        );
        assert!(
            tracker
                .observe(state(after + CONFIRMATION_LIMIT * 2, true, "A", 0.5, false))
                .is_none()
        );
    }
}
