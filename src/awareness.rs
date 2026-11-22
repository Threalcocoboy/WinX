use crate::{
    device::{Device, Key, Value},
    preferences::BatteryOptions,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Low,
    Normal,
    High,
    Critical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Connected,
    Disconnected,
    Noise,
    Setting,
    Media,
    Battery,
    Information,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Windows,
    Device,
    Hotkey,
    Panel,
    Battery,
    Preview,
    Hardware,
    Talk,
}

/// An overlay action is also its icon key. Presentation never infers an icon
/// from a translated title or the notification's broad filtering category.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NoticeAction {
    Connected,
    HeadphonesConnected,
    Disconnected,
    AncOn,
    AncHigh,
    AncMedium,
    AncLow,
    AncOff,
    Transparency,
    AdaptiveAnc,
    Play,
    Pause,
    NextTrack,
    PreviousTrack,
    VolumeUp,
    VolumeDown,
    Mute,
    Unmute,
    BatteryLow,
    BatteryVeryLow,
    BatteryCritical,
    BatteryStatus,
    BatteryUnavailable,
    LatencyOn,
    LatencyOff,
    BassUp,
    BassDown,
    BassOn,
    BassOff,
    Equalizer,
    CustomEqualizer,
    RingLeft,
    RingRight,
    RingBoth,
    RingStop,
    Spatial,
    InEarOn,
    InEarOff,
    PersonalOn,
    PersonalOff,
    DualOn,
    DualOff,
    OverlayEnabled,
    TalkOn,
    TalkOff,
    TalkPartial,
    UpdateAvailable,
}

impl NoticeAction {
    pub fn kind(self) -> Kind {
        use NoticeAction::*;
        match self {
            Connected | HeadphonesConnected => Kind::Connected,
            Disconnected => Kind::Disconnected,
            AncOn | AncHigh | AncMedium | AncLow | AncOff | Transparency | AdaptiveAnc => {
                Kind::Noise
            }
            Play | Pause | NextTrack | PreviousTrack | VolumeUp | VolumeDown | Mute | Unmute => {
                Kind::Media
            }
            BatteryLow | BatteryVeryLow | BatteryCritical => Kind::Battery,
            BatteryStatus | BatteryUnavailable | OverlayEnabled | TalkOn | TalkOff
            | TalkPartial | UpdateAvailable => Kind::Information,
            _ => Kind::Setting,
        }
    }

    fn priority(self) -> Priority {
        use NoticeAction::*;
        match self {
            Connected | HeadphonesConnected | Disconnected => Priority::Low,
            BatteryLow | BatteryVeryLow => Priority::High,
            BatteryCritical => Priority::Critical,
            _ => Priority::Normal,
        }
    }

    pub fn allowed_media(self, settings: crate::preferences::MediaOptions) -> bool {
        use NoticeAction::*;
        match self {
            Play | Pause => settings.play_pause,
            NextTrack | PreviousTrack => settings.track,
            VolumeUp | VolumeDown | Mute | Unmute => settings.volume,
            _ => false,
        }
    }

    pub fn battery_tier(tier: u8) -> Self {
        match tier {
            3 => Self::BatteryCritical,
            2 => Self::BatteryVeryLow,
            _ => Self::BatteryLow,
        }
    }

    pub fn media_title(self) -> Option<&'static str> {
        use NoticeAction::*;
        Some(match self {
            Play => "Playing",
            Pause => "Paused",
            NextTrack => "Next track",
            PreviousTrack => "Previous track",
            VolumeUp => "Volume up",
            VolumeDown => "Volume down",
            Mute => "Volume muted",
            Unmute => "Volume unmuted",
            _ => return None,
        })
    }
}

pub const PREVIEWS: &[(NoticeAction, &str)] = {
    use NoticeAction::*;
    &[
        (Connected, "Connected"),
        (Disconnected, "Disconnected"),
        (AncOn, "Noise cancellation on"),
        (AncHigh, "ANC high"),
        (AncMedium, "ANC medium"),
        (AncLow, "ANC low"),
        (AncOff, "Noise cancellation off"),
        (Transparency, "Transparency mode"),
        (AdaptiveAnc, "Adaptive ANC"),
        (Play, "Playing"),
        (Pause, "Paused"),
        (NextTrack, "Next track"),
        (PreviousTrack, "Previous track"),
        (VolumeUp, "Volume up"),
        (VolumeDown, "Volume down"),
        (Mute, "Volume muted"),
        (Unmute, "Volume unmuted"),
        (BatteryLow, "Battery low"),
        (BatteryVeryLow, "Battery very low"),
        (BatteryCritical, "Battery critical"),
        (LatencyOn, "Low latency on"),
        (LatencyOff, "Low latency off"),
        (BassOn, "Bass enhancement on"),
        (BassOff, "Bass enhancement off"),
        (BassUp, "Bass increased"),
        (BassDown, "Bass decreased"),
        (Equalizer, "EQ preset changed"),
        (CustomEqualizer, "Custom EQ changed"),
        (RingLeft, "Left earbud location"),
        (RingRight, "Right earbud location"),
        (RingBoth, "Earbud location"),
        (RingStop, "Ringing stopped"),
        (Spatial, "Spatial audio"),
        (InEarOn, "In-ear detection on"),
        (InEarOff, "In-ear detection off"),
        (PersonalOn, "Personal sound on"),
        (PersonalOff, "Personal sound off"),
        (DualOn, "Dual connection on"),
        (DualOff, "Dual connection off"),
        (BatteryStatus, "Battery status"),
        (BatteryUnavailable, "Battery unavailable"),
        (OverlayEnabled, "Overlay enabled"),
        (TalkOn, "Talk Mode"),
        (TalkOff, "Talk Mode Off"),
        (TalkPartial, "Talk Mode needs attention"),
        (HeadphonesConnected, "Headphones connected"),
    ]
};

pub fn preview(action: NoticeAction, thresholds: [u8; 3]) -> Option<Notice> {
    use NoticeAction::*;
    let title = PREVIEWS
        .iter()
        .find(|&&(candidate, _)| candidate == action)?
        .1;
    let detail = match action {
        BatteryLow => format!("Preview · {}%", thresholds[0]),
        BatteryVeryLow => format!("Preview · {}%", thresholds[1]),
        BatteryCritical => format!("Preview · {}%", thresholds[2]),
        VolumeUp | VolumeDown | Mute | Unmute => "Preview · 65%".into(),
        Equalizer => "Preview · Balanced".into(),
        _ => "Preview".into(),
    };
    Some(Notice::new(action, Source::Preview, title, detail))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    pub detail: String,
    pub kind: Kind,
    pub priority: Priority,
    pub source: Source,
    pub device: String,
    pub action: NoticeAction,
}

impl Notice {
    pub fn new(
        action: NoticeAction,
        source: Source,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            kind: action.kind(),
            priority: action.priority(),
            source,
            action,
            title: title.into(),
            detail: detail.into(),
            device: String::new(),
        }
    }

    pub fn for_device(mut self, id: &str) -> Self {
        self.device = id.into();
        self
    }
}

#[derive(Default)]
pub struct Notifications {
    active: Option<(Notice, Instant)>,
    deferred: Option<(Notice, Instant, Duration)>,
}

impl Notifications {
    pub fn offer(&mut self, notice: Notice, now: Instant, duration: Duration) {
        if let Some((active, until)) = &self.active
            && *until > now
        {
            if active == &notice {
                return;
            }
            if active.priority > notice.priority
                || active.priority >= Priority::High && active.priority == notice.priority
            {
                let replace = self.deferred.as_ref().is_none_or(|(item, expiry, _)| {
                    *expiry <= now || notice.priority >= item.priority
                });
                if replace {
                    self.deferred = Some((notice, now + Duration::from_secs(8), duration));
                }
                return;
            }
        }
        self.deferred = self
            .deferred
            .take()
            .filter(|(item, expiry, _)| *expiry > now && item.priority >= Priority::High);
        self.active = Some((notice, now + duration));
    }

    pub fn tick(&mut self, now: Instant) -> Option<&Notice> {
        if self.active.as_ref().is_some_and(|(_, until)| now >= *until) {
            self.active = self
                .deferred
                .take()
                .filter(|(_, until, _)| now < *until)
                .map(|(item, _, duration)| (item, now + duration));
        }
        self.active.as_ref().map(|(item, _)| item)
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.active.as_ref().map(|(_, until)| *until)
    }

    pub fn discard_deferred_links(&mut self, id: Option<&str>) {
        self.deferred = self.deferred.take().filter(|(notice, _, _)| {
            !matches!(notice.kind, Kind::Connected | Kind::Disconnected)
                || id.is_some_and(|id| id != notice.device)
        });
    }

    pub fn retain(&mut self, allowed: impl Fn(&Notice) -> bool, now: Instant) {
        if self
            .deferred
            .as_ref()
            .is_some_and(|(item, _, _)| !allowed(item))
        {
            self.deferred = None;
        }
        if let Some((item, until)) = &mut self.active
            && !allowed(item)
        {
            *until = now;
        }
    }
}

#[derive(Debug)]
pub struct LinkSnapshot {
    pub adapters: Vec<u64>,
    pub present: BTreeSet<String>,
    pub observed: Instant,
}

struct Link {
    present: bool,
    qualified: Option<String>,
    absent_since: Option<Instant>,
    entered: Option<Instant>,
    show_after: Option<Instant>,
    connected_notice: Option<Instant>,
    disconnected_notice: Option<Instant>,
}

impl Link {
    fn new(present: bool) -> Self {
        Self {
            present,
            qualified: None,
            absent_since: None,
            entered: None,
            show_after: None,
            connected_notice: None,
            disconnected_notice: None,
        }
    }
}

const ABSENCE: Duration = Duration::from_secs(2);
const BATTERY_WAIT: Duration = Duration::from_millis(900);
const LINK_COOLDOWN: Duration = Duration::from_secs(8);

#[derive(Default)]
pub struct Connections {
    adapters: Option<Vec<u64>>,
    links: BTreeMap<String, Link>,
    reset_at: Option<Instant>,
}

impl Connections {
    pub fn reset(&mut self, now: Instant) {
        self.adapters = None;
        self.links.clear();
        self.reset_at = Some(now);
    }

    pub fn sample(
        &mut self,
        sample: Option<LinkSnapshot>,
        now: Instant,
    ) -> (bool, Vec<(String, Option<Notice>)>) {
        let Some(sample) = sample else {
            self.reset(now);
            return (true, vec![]);
        };
        if self.reset_at.is_some_and(|reset| sample.observed < reset) {
            return (false, vec![]);
        }
        if self.adapters.as_ref() != Some(&sample.adapters) {
            self.adapters = Some(sample.adapters);
            self.links = sample
                .present
                .into_iter()
                .map(|id| (id, Link::new(true)))
                .collect();
            return (true, vec![]);
        }
        let mut changes = Vec::new();
        for id in &sample.present {
            let link = self
                .links
                .entry(id.clone())
                .or_insert_with(|| Link::new(false));
            link.absent_since = None;
            if !link.present {
                link.present = true;
                link.entered = Some(sample.observed);
                changes.push((id.clone(), None));
            }
        }
        for (id, link) in &mut self.links {
            if !link.present || sample.present.contains(id) {
                continue;
            }
            // Require two OS observations separated in time. RFCOMM closes are
            // deliberately absent from this state machine.
            let absent_since = *link.absent_since.get_or_insert(sample.observed);
            if sample.observed.saturating_duration_since(absent_since) < ABSENCE {
                continue;
            }
            link.present = false;
            link.entered = None;
            link.show_after = None;
            let notice = link
                .qualified
                .take()
                .filter(|_| {
                    link.disconnected_notice
                        .is_none_or(|last| now.duration_since(last) >= LINK_COOLDOWN)
                })
                .map(|name| {
                    link.disconnected_notice = Some(now);
                    Notice::new(
                        NoticeAction::Disconnected,
                        Source::Windows,
                        name,
                        "Disconnected",
                    )
                    .for_device(id)
                });
            changes.push((id.clone(), notice));
        }
        self.links.retain(|_, link| {
            link.present
                || [link.connected_notice, link.disconnected_notice]
                    .into_iter()
                    .flatten()
                    .any(|last| now.duration_since(last) < LINK_COOLDOWN)
        });
        (false, changes)
    }

    pub fn confirmed(&mut self, d: &Device, now: Instant) {
        if !d.connected || !d.received_any || d.caps.is_none() {
            return;
        }
        if let Some(link) = self
            .links
            .get_mut(&d.address)
            .filter(|link| link.present && link.absent_since.is_none())
        {
            link.qualified = Some(d.name.clone());
            if link.entered.take().is_some()
                && link
                    .connected_notice
                    .is_none_or(|last| now.duration_since(last) >= LINK_COOLDOWN)
            {
                link.show_after = Some(now + BATTERY_WAIT);
            }
        }
    }

    pub fn control_closed(&mut self, id: &str) {
        if let Some(link) = self.links.get_mut(id) {
            link.show_after = None;
        }
    }

    pub fn take_connected(&mut self, d: &Device, now: Instant) -> Option<Notice> {
        let link = self.links.get_mut(&d.address)?;
        let until = link.show_after?;
        let complete =
            [2, 3, 4].iter().all(|part| d.battery.contains_key(part)) || d.battery.contains_key(&6);
        if !d.connected || link.absent_since.is_some() || (!complete && until > now) {
            return None;
        }
        link.show_after = None;
        link.connected_notice = Some(now);
        let detail = battery_detail(d);
        Some(
            Notice::new(
                connection_action(d),
                Source::Windows,
                &d.name,
                if detail.is_empty() {
                    "Connected".into()
                } else {
                    detail
                },
            )
            .for_device(&d.address),
        )
    }

    pub fn deadline(&self, device: &Device) -> Option<Instant> {
        self.links
            .get(&device.address)
            .filter(|link| link.absent_since.is_none())
            .and_then(|link| link.show_after)
    }

    pub fn verified(&self, d: &Device) -> bool {
        let name = d.name.trim().to_ascii_lowercase();
        d.connected
            && d.received_any
            && d.caps.is_some()
            && (d.known_model
                || ["nothing ", "cmf "]
                    .iter()
                    .any(|prefix| name.starts_with(prefix)))
            && self.links.get(&d.address).is_some_and(|link| {
                link.present && link.qualified.is_some() && link.absent_since.is_none()
            })
    }
}

fn connection_action(d: &Device) -> NoticeAction {
    if d.caps.as_ref().is_some_and(|c| c.ring == "headphone") {
        NoticeAction::HeadphonesConnected
    } else {
        NoticeAction::Connected
    }
}

pub fn part_name(part: u8) -> &'static str {
    match part {
        2 => "Left",
        3 => "Right",
        4 => "Case",
        _ => "Headphones",
    }
}

pub fn battery_detail(d: &Device) -> String {
    d.battery
        .iter()
        .map(|(&part, b)| format!("{} {}%", part_name(part), b.level))
        .collect::<Vec<_>>()
        .join("  ·  ")
}

#[derive(Default, Deserialize, Serialize)]
pub struct BatteryHistory {
    devices: BTreeMap<String, DeviceHistory>,
}

#[derive(Default, Deserialize, Serialize)]
struct DeviceHistory {
    seen_at: u64,
    components: BTreeMap<u8, Component>,
}

#[derive(Default, Deserialize, Serialize)]
struct Component {
    level: u8,
    minimum: u8,
    reported: u8,
}

#[derive(Default)]
pub struct BatteryAlerts {
    pub history: BatteryHistory,
    seen: BTreeSet<u8>,
    revision: u64,
}

impl BatteryAlerts {
    pub fn new(history: BatteryHistory) -> Self {
        Self {
            history,
            ..Self::default()
        }
    }

    pub fn new_session(&mut self) {
        self.reset_observation(0);
    }

    pub fn reset_observation(&mut self, revision: u64) {
        self.seen.clear();
        self.revision = revision;
    }

    pub fn observe(
        &mut self,
        d: &Device,
        options: &BatteryOptions,
        timestamp: u64,
    ) -> (bool, Vec<Notice>) {
        if !d.connected || !d.received_any || self.revision == d.battery_revision {
            return (false, vec![]);
        }
        self.revision = d.battery_revision;
        self.history
            .devices
            .retain(|_, record| timestamp.saturating_sub(record.seen_at) < 30 * 24 * 60 * 60);
        if self.history.devices.len() >= 32
            && !self.history.devices.contains_key(&d.address)
            && let Some(oldest) = self
                .history
                .devices
                .iter()
                .min_by_key(|(_, entry)| entry.seen_at)
                .map(|(id, _)| id.clone())
        {
            self.history.devices.remove(&oldest);
        }
        let record = self.history.devices.entry(d.address.clone()).or_default();
        record.seen_at = timestamp;
        let mut warnings = Vec::new();
        for (&part, battery) in &d.battery {
            let first = self.seen.insert(part);
            let level = battery.level;
            let component = record.components.entry(part).or_insert(Component {
                level,
                minimum: level,
                reported: 0,
            });
            if level >= options.thresholds[0].saturating_add(5).min(100)
                && level
                    >= component
                        .minimum
                        .saturating_add(if battery.charging { 5 } else { 10 })
            {
                component.reported = 0;
                component.minimum = level;
            }
            let previous = component.level;
            component.level = level;
            component.minimum = component.minimum.min(level);
            let monitored = match part {
                2 => options.left,
                3 => options.right,
                4 => options.case_,
                _ => true,
            };
            if battery.charging || !options.enabled || !monitored {
                continue;
            }
            let tier = options
                .thresholds
                .iter()
                .rposition(|threshold| level <= *threshold)
                .map_or(0, |index| index as u8 + 1);
            if tier <= component.reported
                || (tier == 3 && !options.show_critical)
                || (tier < 3 && !options.show_low)
            {
                continue;
            }
            if !(first && tier == 3 || !first && previous > options.thresholds[(tier - 1) as usize])
            {
                continue;
            }
            component.reported = tier;
            warnings.push((part, level, tier));
        }
        warnings.sort_by_key(|&(_, _, tier)| std::cmp::Reverse(tier));
        let make = |tier, text: String| {
            Notice::new(
                NoticeAction::battery_tier(tier),
                Source::Battery,
                match tier {
                    3 => "Battery critical",
                    2 => "Battery very low",
                    _ => "Battery low",
                },
                text,
            )
            .for_device(&d.address)
        };
        if options.combine && warnings.len() > 1 {
            return (
                true,
                vec![make(
                    warnings[0].2,
                    warnings
                        .iter()
                        .map(|&(part, level, _)| format!("{} {level}%", part_name(part)))
                        .collect::<Vec<_>>()
                        .join("  ·  "),
                )],
            );
        }
        (
            true,
            warnings
                .into_iter()
                .map(|(part, level, tier)| {
                    make(tier, format!("{} · {level}% remaining", part_name(part)))
                })
                .collect(),
        )
    }
}

pub fn changed_setting(
    d: &Device,
    key: Key,
    previous: Option<&Value>,
    value: &Value,
    source: Source,
) -> Option<Notice> {
    use NoticeAction::*;
    let (action, title, detail) = match (key, value) {
        (Key::Noise, Value::Byte(n)) => match n {
            1..=3 => (
                if matches!(previous, Some(Value::Byte(1..=4))) {
                    [AncHigh, AncMedium, AncLow][usize::from(n - 1)]
                } else {
                    AncOn
                },
                "Noise cancellation",
                ["High", "Medium", "Low"][usize::from(n - 1)].into(),
            ),
            4 => (AdaptiveAnc, "Adaptive ANC", String::new()),
            5 => (AncOff, "Noise control off", String::new()),
            7 => (Transparency, "Transparency mode", String::new()),
            _ => return None,
        },
        (Key::Bass, Value::Pair(on, wire)) => {
            let step = if d.caps.as_ref()?.bass_steps == 3 {
                5
            } else {
                2
            };
            let (icon, title) = match (*on, previous) {
                (0, _) => (BassOff, "Bass enhancement off"),
                (1, Some(Value::Pair(1, old))) if wire > old => (BassUp, "Bass increased"),
                (1, Some(Value::Pair(1, old))) if wire < old => (BassDown, "Bass decreased"),
                _ => (BassOn, "Bass enhancement on"),
            };
            (
                icon,
                title,
                if *on == 0 {
                    String::new()
                } else {
                    format!("Level {}", wire / step)
                },
            )
        }
        (Key::Preset, Value::Byte(id)) => (
            Equalizer,
            "Equalizer",
            d.caps
                .as_ref()?
                .eq
                .presets
                .iter()
                .find(|preset| preset.id == *id)?
                .label
                .clone(),
        ),
        (Key::CustomEq, Value::Eq(_)) => (CustomEqualizer, "Custom equalizer", "Updated".into()),
        (Key::Spatial, Value::Pair(mode, head)) => (
            Spatial,
            "Spatial audio",
            if *head == 1 {
                "Head tracked"
            } else {
                ["Off", "Fixed", "Concert", "Theatre", "Game"].get(*mode as usize)?
            }
            .to_string(),
        ),
        (Key::LowLatency | Key::InEar | Key::Personal | Key::Dual, Value::Flag(on)) => {
            let (enabled, disabled, label) = match key {
                Key::LowLatency => (LatencyOn, LatencyOff, "Low latency"),
                Key::InEar => (InEarOn, InEarOff, "In-ear detection"),
                Key::Personal => (PersonalOn, PersonalOff, "Personal sound"),
                _ => (DualOn, DualOff, "Dual connection"),
            };
            (
                if *on { enabled } else { disabled },
                label,
                if *on { "On".into() } else { "Off".into() },
            )
        }
        _ => return None,
    };
    Some(Notice::new(action, source, title, detail).for_device(&d.address))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::Battery;

    const DEVICE: &str = "001122334455";

    fn sample(now: Instant, adapter: u64, connected: bool) -> Option<LinkSnapshot> {
        Some(LinkSnapshot {
            adapters: vec![adapter],
            present: if connected {
                BTreeSet::from([DEVICE.into()])
            } else {
                BTreeSet::new()
            },
            observed: now,
        })
    }

    fn device(now: Instant) -> Device {
        let mut d = Device::default();
        d.on_connected("Nothing Ear (3)".into(), DEVICE.into());
        assert!(d.caps.is_some(), "{now:?}");
        d.received_any = true;
        d
    }

    fn battery(d: &mut Device, part: u8, level: u8, charging: bool) {
        d.battery.insert(part, Battery { level, charging });
        d.battery_revision += 1;
    }

    #[test]
    fn startup_baseline_is_silent_even_when_control_socket_connects_later() {
        let start = Instant::now();
        let mut watch = Connections::default();
        assert!(watch.sample(sample(start, 1, true), start).0);
        let t = start + Duration::from_secs(10);
        let d = device(t);
        watch.confirmed(&d, t);
        assert!(watch.take_connected(&d, t + BATTERY_WAIT).is_none());
        assert!(watch.deadline(&d).is_none());
    }

    #[test]
    fn connection_waits_for_real_battery_and_expires_without_missing_values() {
        let start = Instant::now();
        let mut watch = Connections::default();
        watch.sample(sample(start, 1, false), start);
        let t = start + Duration::from_secs(3);
        watch.sample(sample(t, 1, true), t);
        let mut d = device(t);
        watch.confirmed(&d, t);
        battery(&mut d, 2, 84, false);
        battery(&mut d, 3, 0, false);
        assert!(
            watch
                .take_connected(&d, t + Duration::from_millis(300))
                .is_none()
        );
        let notice = watch.take_connected(&d, t + BATTERY_WAIT).unwrap();
        assert_eq!(notice.title, "Nothing Ear (3)");
        assert_eq!(notice.detail, "Left 84%  ·  Right 0%");
        assert_eq!(notice.kind, Kind::Connected);
        watch.confirmed(&d, t + BATTERY_WAIT);
        assert!(
            watch
                .take_connected(&d, t + Duration::from_secs(10))
                .is_none()
        );
    }

    #[test]
    fn complete_battery_can_release_connection_early_and_missing_snapshot_can_stay_text_only() {
        let t = Instant::now();
        let mut watch = Connections::default();
        watch.sample(sample(t, 1, false), t);
        let mut d = device(t);
        watch.sample(sample(t, 1, true), t);
        watch.confirmed(&d, t);
        battery(&mut d, 2, 88, false);
        battery(&mut d, 3, 91, false);
        battery(&mut d, 4, 60, false);
        assert_eq!(
            watch.take_connected(&d, t).unwrap().detail,
            "Left 88%  ·  Right 91%  ·  Case 60%"
        );

        watch.reset(t);
        watch.sample(sample(t, 1, false), t);
        watch.sample(sample(t, 1, true), t);
        d.battery.clear();
        watch.confirmed(&d, t);
        assert_eq!(
            watch.take_connected(&d, t + BATTERY_WAIT).unwrap().detail,
            "Connected"
        );
    }

    #[test]
    fn socket_reconnect_and_transient_native_absence_do_not_trigger_notifications() {
        let t = Instant::now();
        let mut watch = Connections::default();
        let d = device(t);
        watch.sample(sample(t, 1, true), t);
        watch.confirmed(&d, t);
        watch.control_closed(DEVICE);
        assert!(watch.sample(sample(t, 1, false), t).1.is_empty());
        let next = t + Duration::from_secs(1);
        assert!(watch.sample(sample(next, 1, true), next).1.is_empty());
        watch.confirmed(&d, next);
        assert!(watch.take_connected(&d, next + BATTERY_WAIT).is_none());
    }

    #[test]
    fn genuine_absence_is_reported_after_debounce_and_reconnection_is_rate_limited() {
        let t = Instant::now();
        let mut watch = Connections::default();
        let d = device(t);
        watch.sample(sample(t, 1, false), t);
        watch.sample(sample(t, 1, true), t);
        watch.confirmed(&d, t);
        assert!(watch.take_connected(&d, t + BATTERY_WAIT).is_some());
        let missing = t + Duration::from_secs(1);
        assert!(
            watch
                .sample(sample(missing, 1, false), missing)
                .1
                .is_empty()
        );
        let confirmed = missing + ABSENCE;
        let (_, notices) = watch.sample(sample(confirmed, 1, false), confirmed);
        let notice = notices[0].1.as_ref().unwrap();
        assert_eq!(
            (notice.kind, notice.detail.as_str()),
            (Kind::Disconnected, "Disconnected")
        );
        assert!(
            watch
                .sample(sample(confirmed, 1, false), confirmed)
                .1
                .is_empty()
        );
        watch.sample(sample(confirmed, 1, true), confirmed);
        watch.confirmed(&d, confirmed);
        assert!(watch.take_connected(&d, confirmed + BATTERY_WAIT).is_none());
    }

    #[test]
    fn adapter_changes_and_resume_rebaseline_without_disconnection_alerts() {
        let t = Instant::now();
        let mut watch = Connections::default();
        let d = device(t);
        watch.sample(sample(t, 1, true), t);
        watch.confirmed(&d, t);
        assert!(watch.sample(None, t + ABSENCE).1.is_empty());
        let changed = t + ABSENCE;
        assert!(watch.sample(sample(changed, 2, true), changed).0);
        let awake = changed + ABSENCE;
        watch.reset(awake);
        assert!(!watch.sample(sample(changed, 1, false), awake).0);
        assert!(watch.sample(sample(awake, 1, true), awake).0);
        watch.confirmed(&d, awake);
        assert!(watch.take_connected(&d, awake + BATTERY_WAIT).is_none());
    }

    #[test]
    fn connection_notice_defers_behind_critical_battery_and_can_be_cancelled() {
        let now = Instant::now();
        let mut queue = Notifications::default();
        let critical = Notice::new(
            NoticeAction::BatteryCritical,
            Source::Battery,
            "Battery critical",
            "5%",
        );
        let disconnected = Notice::new(
            NoticeAction::Disconnected,
            Source::Windows,
            "Ear",
            "Disconnected",
        )
        .for_device(DEVICE);
        queue.offer(critical.clone(), now, Duration::from_secs(5));
        queue.offer(disconnected.clone(), now, Duration::from_secs(2));
        assert_eq!(queue.tick(now), Some(&critical));
        assert_eq!(
            queue.tick(now + Duration::from_secs(5)),
            Some(&disconnected)
        );
        queue.offer(
            critical,
            now + Duration::from_secs(6),
            Duration::from_secs(5),
        );
        queue.offer(
            disconnected,
            now + Duration::from_secs(6),
            Duration::from_secs(2),
        );
        queue.discard_deferred_links(Some(DEVICE));
        assert!(queue.tick(now + Duration::from_secs(11)).is_none());
    }

    #[test]
    fn battery_thresholds_deduplicate_and_reset_after_charging() {
        let mut d = device(Instant::now());
        let mut alerts = BatteryAlerts::default();
        let options = BatteryOptions::default();
        for (level, charging, count) in [
            (22, false, 0),
            (20, false, 1),
            (20, false, 0),
            (21, false, 0),
            (19, false, 0),
            (10, false, 1),
            (5, false, 1),
            (6, true, 0),
            (26, true, 0),
            (20, false, 1),
        ] {
            battery(&mut d, 2, level, charging);
            assert_eq!(alerts.observe(&d, &options, 1).1.len(), count, "{level}");
            assert!(alerts.observe(&d, &options, 1).1.is_empty());
        }
    }

    #[test]
    fn battery_reconnect_only_allows_unseen_critical_and_combines_earbuds() {
        let mut d = device(Instant::now());
        let mut alerts = BatteryAlerts::default();
        let options = BatteryOptions {
            thresholds: [30, 15, 6],
            ..BatteryOptions::default()
        };
        battery(&mut d, 2, 16, false);
        battery(&mut d, 3, 20, false);
        assert!(alerts.observe(&d, &options, 1).1.is_empty());
        battery(&mut d, 2, 12, false);
        battery(&mut d, 3, 4, false);
        let notices = alerts.observe(&d, &options, 1).1;
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].priority, Priority::Critical);
        assert_eq!(notices[0].detail, "Right 4%  ·  Left 12%");
        let saved = serde_json::to_string(&alerts.history).unwrap();
        let mut alerts = BatteryAlerts::new(serde_json::from_str(&saved).unwrap());
        assert!(alerts.observe(&d, &options, 2).1.is_empty());
    }

    #[test]
    fn external_media_gate_requires_a_live_verified_nothing_or_cmf_session() {
        let now = Instant::now();
        let mut watch = Connections::default();
        let mut d = device(now);
        watch.sample(sample(now, 1, true), now);
        assert!(!watch.verified(&d));
        watch.confirmed(&d, now);
        assert!(watch.verified(&d));
        watch.sample(sample(now, 1, false), now);
        assert!(!watch.verified(&d));
        watch.sample(sample(now, 1, true), now);
        d.disconnect();
        assert!(!watch.verified(&d));
        d.on_connected("Generic Bluetooth headset".into(), DEVICE.into());
        d.caps = Some(crate::models::Capabilities::empty());
        d.received_any = true;
        watch.confirmed(&d, now);
        assert!(!watch.verified(&d));
    }

    #[test]
    fn typed_bass_and_anc_changes_and_previews_use_the_same_action_keys() {
        let d = device(Instant::now());
        let old = Value::Pair(1, 4);
        assert_eq!(
            changed_setting(
                &d,
                Key::Bass,
                Some(&old),
                &Value::Pair(1, 6),
                Source::Hotkey
            )
            .unwrap()
            .action,
            NoticeAction::BassUp
        );
        assert_eq!(
            changed_setting(
                &d,
                Key::Bass,
                Some(&old),
                &Value::Pair(1, 2),
                Source::Device
            )
            .unwrap()
            .action,
            NoticeAction::BassDown
        );
        assert_eq!(
            changed_setting(
                &d,
                Key::Noise,
                Some(&Value::Byte(1)),
                &Value::Byte(4),
                Source::Device
            )
            .unwrap()
            .action,
            NoticeAction::AdaptiveAnc
        );
        for &(action, _) in PREVIEWS {
            let sample = preview(action, [20, 10, 5]).unwrap();
            assert_eq!(
                (sample.action, sample.source, sample.kind),
                (action, Source::Preview, action.kind())
            );
            assert!(sample.device.is_empty());
        }
    }
}
