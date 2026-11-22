use crate::{
    Event,
    media_input_windows::ConsumerInput,
    media_windows::MediaObserver,
    overlay_windows::{self, Overlay},
};
use nothingx_desktop::{
    awareness::{
        self, BatteryAlerts, Connections, Kind, LinkSnapshot, Notice, NoticeAction, Notifications,
        Priority, Source,
    },
    device::{Action, Device, Key},
    hotkeys::{Bindings, Chord, Hotkeys, SHORTCUTS, Shortcut},
    media::{Confirmation, HardwareCommand},
    preferences::{self, BatteryOptions, MediaOptions, OverlayOptions, Preferences},
};
use serde_json::{Value as Json, json};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tao::{event_loop::EventLoopProxy, platform::windows::WindowExtWindows, window::Window};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::Diagnostics::Debug::MessageBeep,
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            MB_ICONEXCLAMATION, PBT_APMRESUMEAUTOMATIC, PBT_APMSUSPEND, SPI_GETCLIENTAREAANIMATION,
            SystemParametersInfoW, WM_INPUT, WM_INPUT_DEVICE_CHANGE, WM_POWERBROADCAST,
            WM_SETTINGCHANGE,
        },
    },
};

pub enum ShortcutEffect {
    None,
    TogglePanel,
    TalkMode,
}

struct WindowEvents {
    hwnd: HWND,
    state: Box<WindowEventState>,
}

struct WindowEventState {
    proxy: EventLoopProxy<Event>,
    consumer: RefCell<ConsumerInput>,
}

impl WindowEvents {
    fn new(window: &Window, proxy: EventLoopProxy<Event>) -> Option<Self> {
        let hwnd = window.hwnd() as HWND;
        let state = Box::new(WindowEventState {
            proxy,
            consumer: RefCell::default(),
        });
        unsafe {
            SetWindowSubclass(
                hwnd,
                Some(window_event),
                1,
                (&*state as *const WindowEventState) as usize,
            )
        }
        .ne(&0)
        .then_some(Self { hwnd, state })
    }
}

impl Drop for WindowEvents {
    fn drop(&mut self) {
        self.state.consumer.get_mut().unregister();
        unsafe {
            RemoveWindowSubclass(self.hwnd, Some(window_event), 1);
        }
    }
}

unsafe extern "system" fn window_event(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    _: usize,
    data: usize,
) -> LRESULT {
    let state = unsafe { &*(data as *const WindowEventState) };
    let event = match (msg, w as u32) {
        (WM_POWERBROADCAST, PBT_APMSUSPEND) => Some(Event::Suspended),
        (WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC) => Some(Event::Resumed),
        (WM_SETTINGCHANGE, _) => Some(Event::DisplayChanged),
        (WM_INPUT, _) => state
            .consumer
            .try_borrow_mut()
            .ok()
            .and_then(|mut input| input.report(l as _, Instant::now()))
            .map(|(generation, action)| Event::HardwareMedia(generation, action)),
        (WM_INPUT_DEVICE_CHANGE, _) => {
            if let Ok(mut input) = state.consumer.try_borrow_mut() {
                input.enumerate();
            }
            Some(Event::MediaDevicesChanged)
        }
        _ => None,
    };
    if let Some(event) = event {
        let _ = state.proxy.send_event(event);
    }
    unsafe { DefSubclassProc(hwnd, msg, w, l) }
}

fn system_reduced_motion() -> bool {
    let mut enabled: i32 = 1;
    let available = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut enabled as *mut i32).cast(),
            0,
        )
    };
    available != 0 && enabled == 0
}

pub struct Awareness {
    directory: PathBuf,
    talk: crate::talk_windows::TalkMode,
    prefs: Preferences,
    shortcuts: Hotkeys,
    connections: Connections,
    battery: BatteryAlerts,
    notices: Notifications,
    overlay: Overlay,
    overlay_revision: u64,
    window_events: Option<WindowEvents>,
    sources: BTreeMap<Key, Source>,
    suppress_device_until: Instant,
    link_quiet_until: Instant,
    suspended: bool,
    media: Option<MediaObserver>,
    media_generation: u64,
    media_context: Option<(String, MediaOptions)>,
    proxy: EventLoopProxy<Event>,
    audio_alert: Option<Notice>,
    recording_until: Option<Instant>,
}

impl Awareness {
    pub fn new(
        directory: PathBuf,
        window: &Window,
        proxy: EventLoopProxy<Event>,
        now: Instant,
    ) -> Self {
        let prefs = Preferences::load(&directory);
        crate::panel::set_accent(prefs.accent());
        crate::media_diagnostics::enable(prefs.media_diagnostics);
        let mut shortcuts = Hotkeys::default();
        shortcuts.sync(&prefs.hotkeys);
        let battery = BatteryAlerts::new(preferences::load_json(
            &directory.join("winx-battery-history.json"),
        ));
        let mut overlay = Overlay::new(now, Some(proxy.clone()));
        overlay.set_reduced(system_reduced_motion());
        Self {
            talk: crate::talk_windows::TalkMode::new(directory.clone(), proxy.clone(), now),
            directory,
            prefs,
            shortcuts,
            connections: Connections::default(),
            battery,
            notices: Notifications::default(),
            overlay,
            overlay_revision: 0,
            window_events: WindowEvents::new(window, proxy.clone()),
            sources: BTreeMap::new(),
            suppress_device_until: now,
            link_quiet_until: now,
            suspended: false,
            media: None,
            media_generation: 0,
            media_context: None,
            proxy,
            audio_alert: None,
            recording_until: None,
        }
    }

    pub fn settings(&self) -> Json {
        let shortcuts: Vec<_> = SHORTCUTS
            .iter()
            .map(|&(action, label)| {
                json!({
                    "action": action, "label": label, "chord": self.prefs.hotkeys.keys.get(&action),
                    "available": self.shortcuts.available(action),
                })
            })
            .collect();
        json!({"overlay": self.prefs.overlay, "battery": self.prefs.battery,
            "theme": self.prefs.settings_theme, "accentColor": self.prefs.accent(), "defaultAccent": Preferences::DEFAULT_ACCENT, "mediaDiagnostics": self.prefs.media_diagnostics,
            "talk": self.prefs.talk, "talkStatus": self.talk.status(),
            "hotkeysEnabled": self.prefs.hotkeys.enabled, "shortcuts": shortcuts, "monitors": overlay_windows::monitors(),
            "previews": awareness::PREVIEWS.iter().map(|(action, label)| json!({"action": action, "label": label})).collect::<Vec<_>>()
        })
    }

    fn commit(&mut self, candidate: Preferences) -> Result<(), &'static str> {
        if candidate.save(&self.directory).is_err() {
            self.shortcuts.sync(&self.prefs.hotkeys);
            return Err("Windows could not save WinX settings.");
        }
        crate::media_diagnostics::enable(candidate.media_diagnostics);
        if self.prefs.accent() != candidate.accent() {
            crate::panel::set_accent(candidate.accent());
            self.visual_settings_changed();
        }
        self.prefs = candidate;
        Ok(())
    }

    fn apply_shortcut_edit(
        &mut self,
        arguments: &[Json],
        prefs: &mut Preferences,
    ) -> Result<(), &'static str> {
        let action: Shortcut = serde_json::from_value(
            arguments
                .first()
                .cloned()
                .ok_or("Missing shortcut action.")?,
        )
        .map_err(|_| "Unknown shortcut action.")?;
        let input = arguments.get(1).ok_or("Missing shortcut.")?;
        if input.is_null() {
            prefs.hotkeys.keys.remove(&action);
            self.shortcuts.remove(action);
            return Ok(());
        }
        let chord: Chord =
            serde_json::from_value(input.clone()).map_err(|_| "Invalid shortcut.")?;
        if !chord.valid() {
            return Err("Choose Ctrl or Alt with a supported key.");
        }
        if prefs
            .hotkeys
            .keys
            .iter()
            .any(|(&other, &key)| other != action && key == chord)
        {
            return Err("This shortcut is already assigned in WinX.");
        }
        if !self.shortcuts.register(action, chord) {
            return Err(
                "Windows could not register that shortcut. The previous assignment was kept.",
            );
        }
        if !prefs.hotkeys.enabled {
            self.shortcuts.remove(action);
        }
        prefs.hotkeys.keys.insert(action, chord);
        Ok(())
    }

    pub fn settings_command(&mut self, args: &[Json], now: Instant) -> Json {
        let result = self.settings_command_result(args, now);
        json!({"error": result.err(), "settings": self.settings()})
    }

    fn settings_command_result(&mut self, args: &[Json], now: Instant) -> Result<(), &'static str> {
        let mut candidate = self.prefs.clone();
        let value = args.get(1).cloned().unwrap_or(Json::Null);
        match args.first().and_then(Json::as_str) {
            Some("overlay") => {
                let preview = value.get("position").and_then(Json::as_str)
                    != serde_json::to_value(candidate.overlay.position)
                        .ok()
                        .as_ref()
                        .and_then(Json::as_str);
                let options: OverlayOptions =
                    serde_json::from_value(value).map_err(|_| "Invalid overlay settings.")?;
                if !options.valid() {
                    return Err("Invalid overlay position, monitor or duration.");
                }
                candidate.overlay = options;
                self.commit(candidate)?;
                self.overlay_revision = self.overlay_revision.wrapping_add(1);
                if preview {
                    self.preview("ancOn", now);
                }
                return Ok(());
            }
            Some("battery") => {
                let options: BatteryOptions =
                    serde_json::from_value(value).map_err(|_| "Invalid battery settings.")?;
                if !options.valid() {
                    return Err("Battery thresholds must descend from low to critical.");
                }
                candidate.battery = options;
            }
            Some("accentColor") => {
                candidate.accent_color = if value.is_null() {
                    None
                } else {
                    Some(
                        value
                            .as_str()
                            .and_then(Preferences::parse_accent)
                            .ok_or("Choose a valid accent color.")?,
                    )
                };
            }
            Some("theme") => {
                candidate.settings_theme =
                    serde_json::from_value(value).map_err(|_| "Unknown appearance.")?;
            }
            Some("mediaDiagnostics") => {
                candidate.media_diagnostics =
                    value.as_bool().ok_or("Invalid diagnostics setting.")?;
            }
            Some("talk") => {
                let options: nothingx_desktop::talk::Options =
                    serde_json::from_value(value).map_err(|_| "Invalid Talk Mode settings.")?;
                if !options.valid() {
                    return Err("Choose a valid volume and fade duration.");
                }
                candidate.talk = options;
                if !options.enabled {
                    self.stop_talk(now);
                }
            }
            Some("recordShortcut") => {
                if value == Json::Bool(true) {
                    self.recording_until = Some(now + Duration::from_secs(20));
                    self.shortcuts.sync(&Bindings {
                        enabled: false,
                        keys: BTreeMap::new(),
                    });
                } else {
                    self.recording_until = None;
                    self.shortcuts.sync(&self.prefs.hotkeys);
                }
                return Ok(());
            }
            Some("shortcut") => {
                self.recording_until = None;
                if let Err(error) = self.apply_shortcut_edit(&args[1..], &mut candidate) {
                    self.shortcuts.sync(&self.prefs.hotkeys);
                    return Err(error);
                }
                self.shortcuts.sync(&candidate.hotkeys);
            }
            Some("resetShortcuts") => {
                candidate.hotkeys.keys = Bindings::default().keys;
                self.shortcuts.sync(&Bindings {
                    enabled: false,
                    keys: BTreeMap::new(),
                });
                self.shortcuts.sync(&candidate.hotkeys);
            }
            Some("hotkeysEnabled") => {
                candidate.hotkeys.enabled = value.as_bool().ok_or("Invalid shortcut setting.")?;
                self.shortcuts.sync(&candidate.hotkeys);
            }
            Some("preview") => {
                self.preview(value.as_str().unwrap_or("connected"), now);
                return Ok(());
            }
            _ => return Err("Unknown WinX setting."),
        }
        self.commit(candidate)
    }

    fn preview(&mut self, example: &str, now: Instant) {
        if let Ok(action) = serde_json::from_value::<NoticeAction>(json!(example))
            && let Some(notice) = awareness::preview(action, self.prefs.battery.thresholds)
        {
            self.offer(notice, now);
        }
    }

    pub fn device_ready(&self, device: &Device) -> bool {
        self.connections.verified(device) && !self.suspended
    }

    pub fn preferred_device(&self) -> u64 {
        self.prefs
            .preferred_device
            .as_ref()
            .and_then(|address| u64::from_str_radix(address, 16).ok())
            .unwrap_or(0)
    }

    pub fn select_device(&mut self, address: u64) -> Result<(), &'static str> {
        let mut prefs = self.prefs.clone();
        prefs.preferred_device = (address != 0).then(|| format!("{address:012X}"));
        self.commit(prefs)
    }

    pub fn complete_onboarding(&mut self) -> Result<(), &'static str> {
        let mut prefs = self.prefs.clone();
        prefs.onboarding_version = Preferences::TOUR_VERSION;
        self.commit(prefs)
    }

    pub fn talk_status(&self) -> Json {
        self.talk.status()
    }

    pub fn talk_toggle(&mut self, device: &mut Device, now: Instant) {
        self.talk.toggle(
            device,
            self.connections.verified(device) && !self.suspended,
            self.prefs.talk,
            now,
        );
    }

    pub fn retry_talk(&mut self, device: &Device, now: Instant) {
        self.talk.retry(device, now);
    }
    pub fn archive_talk(&mut self, now: Instant) -> std::io::Result<()> {
        self.talk.archive(now)
    }

    pub fn stop_talk(&mut self, now: Instant) {
        self.talk.stop(now, false);
    }

    pub fn talk_inactive(&self) -> bool {
        self.talk.inactive()
    }

    pub fn talk_transitioning(&self) -> bool {
        self.talk.transitioning()
    }

    pub fn safe_to_update(&self) -> bool {
        self.talk.safe_to_update()
    }

    pub fn save_for_update(&self) -> Result<(), String> {
        self.prefs
            .save(&self.directory)
            .map_err(|_| "WinX could not save its settings. The installer has not started.".into())
    }

    pub fn update_notice(&mut self, version: &str, now: Instant) {
        self.offer(
            Notice::new(
                NoticeAction::UpdateAvailable,
                Source::Windows,
                format!("WinX {version} available"),
                "A new update is ready",
            ),
            now,
        );
    }

    pub fn dismiss_update_notice(&mut self, now: Instant) {
        self.notices
            .retain(|notice| notice.action != NoticeAction::UpdateAvailable, now);
    }

    pub fn talk_audio(
        &mut self,
        generation: u64,
        result: crate::talk_audio_windows::Result,
        now: Instant,
    ) {
        self.talk.audio_result(generation, result, now);
    }

    pub fn visual_settings_changed(&mut self) {
        self.overlay_revision = self.overlay_revision.wrapping_add(1);
        self.overlay.set_reduced(system_reduced_motion());
    }

    pub fn motion(&mut self, reduced: bool) {
        self.overlay.set_reduced(reduced || system_reduced_motion());
    }

    pub fn suspend(&mut self, suspended: bool, battery_revision: u64, now: Instant) {
        self.suspended = suspended;
        self.link_quiet_until = now;
        self.connections.reset(now);
        self.battery.reset_observation(battery_revision);
        self.sources.clear();
        self.notices = Notifications::default();
        self.overlay.hide();
        self.audio_alert = None;
        self.stop_media();
        self.refresh(now);
    }

    pub fn refresh(&mut self, now: Instant) {
        self.suppress_device_until = now + Duration::from_secs(3);
    }

    pub fn link_snapshot(&mut self, snapshot: Option<LinkSnapshot>, d: &Device, now: Instant) {
        if self.suspended || now < self.link_quiet_until {
            return;
        }
        let (new_baseline, effects) = self.connections.sample(snapshot, now);
        if new_baseline {
            self.notices.discard_deferred_links(None);
        }
        self.connections.confirmed(d, now);
        for (id, event) in effects {
            self.notices.discard_deferred_links(Some(&id));
            if let Some(notice) = event {
                self.offer(notice, now);
            }
        }
    }

    pub fn control_connected(&mut self, now: Instant) {
        self.stop_media();
        self.sources.clear();
        self.battery.new_session();
        self.refresh(now);
    }

    pub fn control_closed(&mut self, id: &str) {
        self.stop_media();
        self.connections.control_closed(id);
        self.sources.clear();
        self.battery.new_session();
    }

    pub fn apply_action(&mut self, d: &mut Device, action: Action, source: Source, now: Instant) {
        if let Action::Set(key, value) = &action {
            let (key, previous) = (*key, d.pending_value(*key).cloned());
            let target = value.clone();
            d.action(action, now);
            if previous.as_ref() != Some(&target) && d.pending_value(key) == Some(&target) {
                self.sources.insert(key, source);
                if key == Key::Dual && d.caps.as_ref().is_some_and(|caps| caps.dual_restarts) {
                    self.link_quiet_until = now + Duration::from_secs(8);
                    self.connections.reset(now);
                    self.notices.discard_deferred_links(None);
                }
            }
        } else {
            let previous = d.ringing;
            let ring_request = matches!(action, Action::Ring(_) | Action::StopRing);
            d.action(action, now);
            if ring_request && d.ringing != previous {
                use NoticeAction::*;
                let (action, title) = match d.ringing {
                    Some(2) => (RingLeft, "Ring left earbud"),
                    Some(3) => (RingRight, "Ring right earbud"),
                    Some(_) => (RingBoth, "Ring earbuds"),
                    None => (RingStop, "Stop ringing"),
                };
                self.offer(
                    Notice::new(action, source, title, "Requested").for_device(&d.address),
                    now,
                );
            }
        }
    }

    pub fn receive(&mut self, d: &mut Device, bytes: &[u8], now: Instant) {
        let before = d.values.clone();
        d.receive(bytes);
        if self.suspended {
            return;
        }
        self.connections.confirmed(d, now);
        for (&key, value) in &d.values {
            if Some(value) == before.get(&key) || d.is_pending(key) || self.talk.owns_pending(key) {
                continue;
            }
            let requested = self.sources.remove(&key);
            let source = match requested {
                Some(source) => source,
                None if now >= self.suppress_device_until && before.contains_key(&key) => {
                    Source::Device
                }
                _ => continue,
            };
            if let Some(notice) =
                awareness::changed_setting(d, key, before.get(&key), value, source)
            {
                self.offer(notice, now);
            }
        }
        if self.prefs.battery.quiet && overlay_windows::quiet(true) {
            // Still track fresh readings, but do not spend a warning on an
            // overlay that Windows is currently suppressing.
            let mut silent = self.prefs.battery.clone();
            silent.enabled = false;
            self.battery.observe(d, &silent, unix_time());
        } else {
            let (updated, warnings) = self.battery.observe(d, &self.prefs.battery, unix_time());
            if updated {
                let _ = preferences::save_json(
                    &self.directory.join("winx-battery-history.json"),
                    &self.battery.history,
                );
            }
            for notice in warnings {
                self.offer(notice, now);
            }
        }
    }

    pub fn hotkey(
        &mut self,
        id: i32,
        d: &mut Device,
        now: Instant,
        allow_talk: bool,
    ) -> ShortcutEffect {
        if self.recording_until.is_some() {
            return ShortcutEffect::None;
        }
        let Some(action) = self.shortcuts.action(id) else {
            return ShortcutEffect::None;
        };
        match action {
            Shortcut::Panel => return ShortcutEffect::TogglePanel,
            Shortcut::TalkMode => {
                if allow_talk {
                    self.talk_toggle(d, now);
                }
                return ShortcutEffect::TalkMode;
            }
            Shortcut::Overlay => {
                let mut prefs = self.prefs.clone();
                prefs.overlay.enabled = !prefs.overlay.enabled;
                if self.commit(prefs).is_ok() && self.prefs.overlay.enabled {
                    self.offer(
                        Notice::new(
                            NoticeAction::OverlayEnabled,
                            Source::Hotkey,
                            "WinX overlay",
                            "Enabled",
                        ),
                        now,
                    );
                }
            }
            Shortcut::Battery => {
                if d.connected && !d.battery.is_empty() {
                    self.offer(
                        Notice::new(
                            NoticeAction::BatteryStatus,
                            Source::Hotkey,
                            &d.name,
                            awareness::battery_detail(d),
                        )
                        .for_device(&d.address),
                        now,
                    );
                } else {
                    self.offer(
                        Notice::new(
                            NoticeAction::BatteryUnavailable,
                            Source::Hotkey,
                            "Battery unavailable",
                            "",
                        ),
                        now,
                    );
                }
            }
            _ => {
                if let Some(command) = action.device_action(d) {
                    self.apply_action(d, command, Source::Hotkey, now);
                }
            }
        }
        ShortcutEffect::None
    }

    pub fn hardware_media(&self, generation: u64, command: HardwareCommand, d: &Device) {
        let verified = self.media_allowed(d);
        let accepted = generation == self.media_generation
            && verified
            && command.action.enabled(self.prefs.overlay.media)
            && self.media.is_some();
        crate::media_diagnostics::record(format_args!(
            "hardware command {:?} source={:?} confidence=pnp verified={verified} accepted={accepted}",
            command.action, command.source
        ));
        if generation == self.media_generation
            && verified
            && command.action.enabled(self.prefs.overlay.media)
            && let Some(observer) = &self.media
        {
            observer.command(command);
        }
    }

    pub fn media(&mut self, generation: u64, result: Confirmation, d: &Device, now: Instant) {
        if generation == self.media_generation
            && self.media.is_some()
            && self.media_allowed(d)
            && let Some(title) = result.action.media_title()
        {
            crate::media_diagnostics::record(format_args!(
                "confirmed action {:?}; submitted to overlay priority and quiet-state filters",
                result.action
            ));
            self.offer(
                Notice::new(result.action, Source::Hardware, title, result.detail)
                    .for_device(&d.address),
                now,
            );
        }
    }

    fn media_allowed(&self, d: &Device) -> bool {
        !self.suspended
            && !self.talk.transitioning()
            && self.prefs.overlay.enabled
            && self.prefs.overlay.media.enabled()
            && self.connections.verified(d)
    }

    fn stop_media(&mut self) {
        if self.media_context.take().is_some() {
            self.media_generation = self.media_generation.wrapping_add(1);
            self.media.take();
            crate::media_diagnostics::record(format_args!(
                "media context cleared; pending events invalidated"
            ));
            if let Some(window) = &self.window_events {
                window.state.consumer.borrow_mut().unregister();
            }
        }
    }

    pub fn media_devices_changed(&mut self) {
        if self.media_context.is_some() {
            self.media_generation = self.media_generation.wrapping_add(1);
            self.media.take();
            if let Some(window) = &self.window_events {
                window
                    .state
                    .consumer
                    .borrow_mut()
                    .generation(self.media_generation);
            }
        }
    }

    fn sync_media(&mut self, d: &Device) {
        if !self.media_allowed(d) {
            self.stop_media();
            return;
        }
        let options = self.prefs.overlay.media;
        if self
            .media_context
            .as_ref()
            .is_none_or(|(address, old)| address != &d.address || *old != options)
        {
            self.stop_media();
            self.media_generation = self.media_generation.wrapping_add(1);
            self.media_context = Some((d.address.clone(), options));
            if let Some(window) = &self.window_events {
                window.state.consumer.borrow_mut().register(
                    window.hwnd,
                    self.media_generation,
                    &d.address,
                );
            }
        }
        if self.media.is_none()
            && self
                .window_events
                .as_ref()
                .is_some_and(|window| window.state.consumer.borrow().registered())
        {
            self.media = Some(MediaObserver::start(
                self.proxy.clone(),
                self.media_generation,
                options,
            ));
        }
    }

    fn offer(&mut self, notice: Notice, now: Instant) {
        let preview = notice.source == Source::Preview;
        let respect = notice.kind == Kind::Battery && self.prefs.battery.quiet;
        if self.suspended
            || (!preview && overlay_windows::quiet(respect))
            || !visible(&self.prefs, &notice)
        {
            return;
        }
        let seconds = if notice.action == NoticeAction::UpdateAvailable {
            12
        } else if notice.kind == Kind::Battery {
            self.prefs.battery.priority_seconds
        } else {
            self.prefs.overlay.seconds
        };
        self.notices
            .offer(notice, now, Duration::from_secs(seconds));
    }

    pub fn tick(&mut self, d: &mut Device, now: Instant) {
        if self.recording_until.is_some_and(|until| now >= until) {
            self.recording_until = None;
            self.shortcuts.sync(&self.prefs.hotkeys);
        }
        self.talk
            .tick(d, self.connections.verified(d) && !self.suspended, now);
        if let Some((action, title, detail)) = self.talk.feedback() {
            self.offer(Notice::new(action, Source::Talk, title, detail), now);
        }
        self.sync_media(d);
        self.sources.retain(|key, _| d.is_pending(*key));
        if let Some(notice) = self.connections.take_connected(d, now) {
            self.offer(notice, now);
        }
        let media_allowed = self.media_allowed(d) && self.media.is_some();
        self.notices.retain(
            |notice| {
                visible(&self.prefs, notice)
                    && (notice.kind != Kind::Media
                        || notice.source == Source::Preview
                        || media_allowed)
            },
            now,
        );
        let notice = self.notices.tick(now);
        let sound = notice
            .filter(|item| item.priority == Priority::Critical && item.source == Source::Battery);
        if sound != self.audio_alert.as_ref() {
            if sound.is_some() && self.prefs.battery.sound {
                unsafe {
                    MessageBeep(MB_ICONEXCLAMATION);
                }
            }
            self.audio_alert = sound.cloned();
        }
        self.overlay
            .render(notice, &self.prefs.overlay, self.overlay_revision, now);
    }

    pub fn deadline(&self, d: &Device, now: Instant) -> Option<Instant> {
        [
            self.connections.deadline(d),
            self.notices.deadline(),
            self.overlay.deadline(now),
            self.recording_until,
            self.talk.deadline(now),
        ]
        .into_iter()
        .flatten()
        .min()
    }
}

fn visible(prefs: &Preferences, notice: &Notice) -> bool {
    if notice.action == NoticeAction::UpdateAvailable {
        return true;
    }
    if notice.source == Source::Preview {
        return true;
    }
    if notice.kind == Kind::Battery {
        return prefs.battery.enabled
            && if notice.priority == Priority::Critical {
                prefs.battery.show_critical
            } else {
                prefs.battery.show_low
            };
    }
    prefs.overlay.enabled
        && (notice.source != Source::Talk || prefs.talk.overlay)
        && match notice.kind {
            Kind::Connected => prefs.overlay.connected,
            Kind::Disconnected => prefs.overlay.disconnected,
            Kind::Noise => prefs.overlay.noise && notice.source != Source::Panel,
            Kind::Setting => prefs.overlay.controls && notice.source != Source::Panel,
            Kind::Media => {
                notice.source == Source::Hardware
                    && notice.action.allowed_media(prefs.overlay.media)
            }
            _ => true,
        }
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_preferences_separate_hardware_feedback_and_explicit_previews() {
        let mut prefs = Preferences::default();
        let foreign = Notice::new(NoticeAction::Pause, Source::Windows, "Paused", "");
        let hardware = Notice::new(NoticeAction::Pause, Source::Hardware, "Paused", "");
        assert!(!visible(&prefs, &foreign));
        assert!(visible(&prefs, &hardware));
        prefs.overlay.media.play_pause = false;
        assert!(!visible(&prefs, &hardware));
        prefs.overlay.enabled = false;
        assert!(visible(
            &prefs,
            &awareness::preview(NoticeAction::Pause, [20, 10, 5]).unwrap()
        ));
    }
}
