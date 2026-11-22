#![cfg_attr(windows, windows_subsystem = "windows")]
#[cfg(windows)]
mod awareness_windows;
#[cfg(windows)]
mod media_diagnostics;
#[cfg(windows)]
mod media_input_windows;
#[cfg(windows)]
mod media_windows;
#[cfg(windows)]
mod overlay_icons;
#[cfg(windows)]
mod overlay_theme;
#[cfg(windows)]
mod overlay_windows;
mod panel;
mod platform;
#[cfg(windows)]
mod pnp_windows;
#[cfg(windows)]
mod talk_audio_windows;
#[cfg(windows)]
mod talk_windows;
mod transport;
#[cfg(windows)]
mod update_ui_windows;

#[cfg(not(windows))]
use nothingx_desktop::updater::{self, InstallOutcome, ReleaseInfo};
use nothingx_desktop::{
    APP_NAME,
    device::{Action, Device},
    ui_state,
    updater::UpdateStatus,
};
use serde_json::{Value as Json, json};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tao::{
    event::{Event as TaoEvent, StartCause, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
};
use tray_icon::{
    MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent,
    menu::{CheckMenuItem, Menu, MenuEvent},
};

#[derive(Debug)]
pub enum Event {
    Connected(String, String),
    Data(Vec<u8>),
    Disconnected,
    Unrecognized(Option<String>),
    #[cfg(not(windows))]
    UpdateStatus(UpdateStatus),
    #[cfg(not(windows))]
    UpdateRestart,
    #[cfg(windows)]
    WinXUpdate(update_ui_windows::Message),
    #[cfg(windows)]
    UpdateNoticeAction(crate::overlay_windows::UpdateAction),
    #[cfg(target_os = "macos")]
    Log(String),
    #[cfg(target_os = "macos")]
    TransportExited,
    Tray(TrayIconEvent),
    #[cfg(windows)]
    TrayMenu(String),
    #[cfg(windows)]
    DisplayChanged,
    #[cfg(windows)]
    Resumed,
    #[cfg(windows)]
    Suspended,
    #[cfg(windows)]
    Hotkey(i32),
    #[cfg(windows)]
    LinkSnapshot(Option<nothingx_desktop::awareness::LinkSnapshot>),
    #[cfg(windows)]
    BluetoothDevices(Vec<transport::Candidate>),
    #[cfg(windows)]
    Media(u64, nothingx_desktop::media::Confirmation),
    #[cfg(windows)]
    HardwareMedia(u64, nothingx_desktop::media::HardwareCommand),
    #[cfg(windows)]
    MediaDevicesChanged,
    #[cfg(windows)]
    TalkAudio(u64, talk_audio_windows::Result),
    Ipc(String),
    #[cfg(windows)]
    SettingsIpc(String),
    #[cfg(target_os = "macos")]
    OutsideClick,
    Reopen,
}

#[cfg(not(windows))]
fn spawn_update_check(proxy: EventLoopProxy<Event>, manual: bool) {
    std::thread::spawn(move || {
        let next = match updater::check_latest() {
            Ok(Some(release)) => UpdateStatus::Available(release),
            Ok(None) => {
                if manual {
                    UpdateStatus::UpToDate
                } else {
                    UpdateStatus::Idle
                }
            }
            Err(message) => UpdateStatus::Error { message, manual },
        };
        let _ = proxy.send_event(Event::UpdateStatus(next));
    });
}

#[cfg(not(windows))]
fn spawn_update_install(proxy: EventLoopProxy<Event>, release: ReleaseInfo) {
    let data_dir = platform::data_dir();
    std::thread::spawn(move || {
        let progress_proxy = proxy.clone();
        match updater::download_and_install(&release, &data_dir, move |status| {
            let _ = progress_proxy.send_event(Event::UpdateStatus(status));
        }) {
            Ok(InstallOutcome::RestartNow) => {
                let _ = proxy.send_event(Event::UpdateRestart);
            }
            Ok(InstallOutcome::OpenedManually) => {
                let _ = proxy.send_event(Event::UpdateStatus(UpdateStatus::OpenedInstaller {
                    version: release.version,
                }));
            }
            Err(message) => {
                let _ = proxy.send_event(Event::UpdateStatus(UpdateStatus::Error {
                    message,
                    manual: true,
                }));
            }
        }
    });
}

fn reopen(
    d: &mut Device,
    now: Instant,
    bluetooth: &mut Option<transport::Transport>,
    panel: &Option<panel::Panel>,
    tray: &Option<tray_icon::TrayIcon>,
    ignore_blur_until: &mut Instant,
) {
    d.action(Action::Refresh, now);
    if !d.connected
        && let Some(t) = bluetooth
    {
        t.refresh();
    }
    if let (Some(p), Some(tray)) = (panel, tray) {
        *ignore_blur_until = now + Duration::from_millis(250);
        p.show(tray);
    }
}

fn write_status(
    path: &std::path::Path,
    state: &Json,
    d: &Device,
    visibility: (bool, bool),
    ready: bool,
) {
    let Ok(bytes) = serde_json::to_vec_pretty(&json!({
        "device": state,
        "message": d.message,
        "popoverVisible": visibility.0,
        "popoverOpen": visibility.1,
        "rendererReady": ready,
    })) else {
        return;
    };
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, &bytes).is_err() || std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::write(path, bytes);
    }
}

fn start_transport(
    proxy: EventLoopProxy<Event>,
    #[cfg(windows)] preferred: u64,
) -> Result<transport::Transport, String> {
    transport::Transport::start(
        proxy,
        #[cfg(windows)]
        preferred,
    )
}

#[cfg(windows)]
fn apply_settings_theme(window: &panel::Panel, theme: &Json) {
    window.window.set_theme(match theme.as_str() {
        Some("light") => Some(tao::window::Theme::Light),
        Some("dark") => Some(tao::window::Theme::Dark),
        _ => None,
    });
}

#[cfg(windows)]
fn show_settings(
    target: &tao::event_loop::EventLoopWindowTarget<Event>,
    proxy: &EventLoopProxy<Event>,
    settings: &mut Option<panel::Panel>,
    awareness: Option<&awareness_windows::Awareness>,
    route: &str,
) {
    if settings.is_none() {
        match panel::Panel::new_window(target, proxy.clone(), true) {
            Ok(window) => *settings = Some(window),
            Err(error) => platform::show_information("WinX Settings", &error.to_string()),
        }
    }
    if let Some(window) = settings {
        if let Some(awareness) = awareness {
            apply_settings_theme(window, &awareness.settings()["theme"]);
        }
        window.show_settings();
        window.route_panel(route);
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.iter().any(|s| s == "--version") {
        println!("{APP_NAME} {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.iter().any(|s| s == "--help") {
        println!(
            "{APP_NAME}\n--version\n--open\n--status-file PATH\n--quit-after SECONDS\n--no-bluetooth\n--media-inventory PATH (Windows)"
        );
        return;
    }
    #[cfg(windows)]
    if let Some(path) = args
        .iter()
        .position(|arg| arg == "--media-inventory")
        .and_then(|i| args.get(i + 1))
    {
        let devices = transport::candidates().unwrap_or_default();
        let addresses: Vec<_> = devices
            .iter()
            .filter(|device| device.linked && device.compatible_audio())
            .map(|device| format!("{:012X}", device.address))
            .collect();
        let reports: Vec<_> = addresses.iter().map(|address| {
            let (consumer, matched, usb) = media_input_windows::inventory(address);
            json!({"consumer_collections": consumer, "headset_collections": matched, "external_usb_collections": usb})
        }).collect();
        let (consumer, _, usb) = media_input_windows::inventory("");
        let value = json!({"recognized_linked_headsets": addresses.len(), "consumer_collections": consumer, "external_usb_collections": usb, "headsets": reports});
        if std::fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).is_err() {
            std::process::exit(1);
        }
        return;
    }
    let quit_at = args
        .iter()
        .position(|s| s == "--quit-after")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u64>().ok())
        .map(|s| Instant::now() + Duration::from_secs(s));
    let status_path = args
        .iter()
        .position(|s| s == "--status-file")
        .and_then(|i| args.get(i + 1))
        .map(std::path::PathBuf::from);
    let auto_open = args.iter().any(|s| s == "--open");
    let bluetooth_enabled = !args.iter().any(|s| s == "--no-bluetooth");
    let _lock = match platform::single_instance() {
        Ok(f) => f,
        Err(e) => {
            if !args.iter().any(|arg| arg == "--startup") {
                platform::notify_running_instance();
            }
            eprintln!("App already running or lock unavailable: {e}");
            return;
        }
    };
    let mut builder = EventLoopBuilder::<Event>::with_user_event();
    #[cfg(windows)]
    let reopen_proxy =
        Arc::new(std::sync::OnceLock::<tao::event_loop::EventLoopProxy<Event>>::new());
    #[cfg(windows)]
    {
        use tao::platform::windows::EventLoopBuilderExtWindows;
        use windows_sys::Win32::UI::WindowsAndMessaging::{MSG, WM_DISPLAYCHANGE, WM_HOTKEY};
        let hook_proxy = reopen_proxy.clone();
        builder.with_msg_hook(move |msg| {
            let msg = unsafe { &*(msg as *const MSG) };
            let event = match msg.message {
                platform::WM_REOPEN => Some(Event::Reopen),
                WM_DISPLAYCHANGE => Some(Event::DisplayChanged),
                WM_HOTKEY if msg.hwnd.is_null() => Some(Event::Hotkey(msg.wParam as i32)),
                _ => None,
            };
            if let (Some(proxy), Some(event)) = (hook_proxy.get(), event) {
                let _ = proxy.send_event(event);
            }
            msg.message == platform::WM_REOPEN || msg.message == WM_HOTKEY && msg.hwnd.is_null()
        });
    }
    let event_loop = builder.build();
    #[cfg(target_os = "macos")]
    let event_loop = {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        let mut loop_ = event_loop;
        loop_.set_activation_policy(ActivationPolicy::Accessory);
        loop_
    };
    let proxy = event_loop.create_proxy();
    #[cfg(windows)]
    let _ = reopen_proxy.set(proxy.clone());
    let popup_selection = Arc::new(Mutex::new(None::<String>));
    let selected = popup_selection.clone();
    #[cfg(windows)]
    let menu_proxy = proxy.clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        if e.id.0.starts_with("popup:") {
            *selected.lock().unwrap() = Some(e.id.0);
            return;
        }
        #[cfg(windows)]
        let _ = menu_proxy.send_event(Event::TrayMenu(e.id.0));
    }));
    let tray_proxy = proxy.clone();
    TrayIconEvent::set_event_handler(Some(move |e| {
        let _ = tray_proxy.send_event(Event::Tray(e));
    }));
    let mut d = Device::default();
    #[cfg(not(windows))]
    let mut update_status = UpdateStatus::Idle;
    #[cfg(windows)]
    let update_status = UpdateStatus::Idle;
    #[cfg(not(windows))]
    let mut startup_update_check: Option<Instant> = None;
    #[cfg(not(windows))]
    let mut update_notice_until: Option<Instant> = None;
    #[cfg(windows)]
    let mut updates =
        update_ui_windows::Updates::new(Instant::now(), quit_at.is_none() && status_path.is_none());
    #[cfg(windows)]
    let mut update_signature = Json::Null;
    #[cfg(windows)]
    let mut settings_route = "general";
    let mut tray = None;
    let mut panel: Option<panel::Panel> = None;
    #[cfg(windows)]
    let mut settings_window: Option<panel::Panel> = None;
    let mut bluetooth: Option<transport::Transport> = None;
    let mut restart = None;
    let mut ready = false;
    let mut previous_ready = false;
    let mut last_hidden = Instant::now() - Duration::from_secs(1);
    let mut signature = Json::Null;
    let mut ignore_blur_until = Instant::now();
    let mut previous_visibility = (false, false);
    #[cfg(windows)]
    let mut awareness: Option<awareness_windows::Awareness> = None;
    #[cfg(windows)]
    let mut exit_after_talk = None;
    #[cfg(windows)]
    let mut talk_signature = Json::Null;
    #[cfg(windows)]
    let mut listed_devices = Vec::<transport::Candidate>::new();
    #[cfg(windows)]
    let mut tour_open = false;
    #[cfg(windows)]
    let mut tour_demo_live = false;
    #[cfg(windows)]
    let launch_tour = quit_at.is_none()
        && status_path.is_none()
        && !args.iter().any(|arg| arg == "--startup")
        && nothingx_desktop::preferences::Preferences::should_onboard(&platform::data_dir());
    #[cfg(windows)]
    let startup_item = CheckMenuItem::with_id(
        "tray:startup",
        "Start with Windows",
        true,
        platform::starts_with_windows(),
        None,
    );
    #[cfg(windows)]
    let app_args = args;
    event_loop.run(move |event, target, flow| {
        #[cfg(windows)]
        let dismiss_on_blur = !tour_open;
        #[cfg(not(windows))]
        let dismiss_on_blur = true;

        let now = Instant::now();
        let mut changed = false;
        let mut quit = false;
        match event {
            TaoEvent::NewEvents(StartCause::Init) => {
                let tray_builder = TrayIconBuilder::new()
                    .with_icon(panel::tray_icon())
                    .with_icon_as_template(false)
                    .with_tooltip(APP_NAME)
                    .with_menu_on_left_click(false);
                #[cfg(windows)]
                let tray_builder = {
                    use tray_icon::menu::{MenuItem, PredefinedMenuItem};

                    let menu = Menu::new();
                    menu.append_items(&[
                        &MenuItem::with_id("tray:open", "Open", true, None),
                        &startup_item,
                        &MenuItem::with_id("tray:updates", "Updates", true, None),
                        &MenuItem::with_id("tray:about", "About", true, None),
                        &PredefinedMenuItem::separator(),
                        &MenuItem::with_id("tray:quit", "Quit", true, None),
                    ])
                    .expect("tray menu");
                    tray_builder.with_menu(Box::new(menu))
                };
                tray = Some(tray_builder.build().expect("tray icon"));
                panel =
                    Some(panel::Panel::new(target, proxy.clone()).expect("system webview panel"));
                if let (Some(tray), Some(p)) = (&tray, &panel) {
                    panel::update_tray_icon(tray, &p.window);
                    #[cfg(windows)]
                    {
                        awareness = Some(awareness_windows::Awareness::new(
                            platform::data_dir(),
                            &p.window,
                            proxy.clone(),
                            now,
                        ));
                    }
                }
                changed = true;
                #[cfg(not(windows))]
                if quit_at.is_none() {
                    startup_update_check = Some(now + Duration::from_secs(2));
                }
                if bluetooth_enabled { match start_transport(proxy.clone(),
                    #[cfg(windows)]
                    awareness.as_ref().map_or(0, |a| a.preferred_device()),
                ) {
                    Ok(t) => bluetooth = Some(t),
                    Err(e) => {
                        d.message = e;
                        restart = Some(now + Duration::from_secs(10));
                    }
                } }
            }
            #[cfg(windows)]
            TaoEvent::WindowEvent { window_id, event, .. }
                if settings_window.as_ref().is_some_and(|window| window.window.id() == window_id) => {
                let window = settings_window.as_ref().unwrap();
                match event {
                    WindowEvent::CloseRequested => window.window.set_visible(false),
                    WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => window.resize_child(),
                    WindowEvent::ThemeChanged(_) => window.update_accent(),
                    _ => {}
                }
            }
            TaoEvent::WindowEvent {
                event: WindowEvent::Focused(false),
                ..
            } => {
                if dismiss_on_blur
                    && let Some(p) = &panel
                    && p.is_open()
                    && !p.has_focus()
                    && now >= ignore_blur_until
                {
                    p.hide();
                    last_hidden = now;
                }
            }
            TaoEvent::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                if let Some(p) = &panel {
                    #[cfg(windows)]
                    if tour_open && let Some(awareness) = &mut awareness {
                        awareness.stop_talk(now);
                    }
                    p.hide();
                    last_hidden = now;
                }
            }
            #[cfg(windows)]
            TaoEvent::WindowEvent { event: WindowEvent::Resized(_), .. } => {
                if let Some(p) = &panel { p.tour_resized(); }
            }
            TaoEvent::WindowEvent {
                event: WindowEvent::ThemeChanged(_),
                ..
            } => {
                if let (Some(tray), Some(p)) = (&tray, &panel) {
                    panel::update_tray_icon(tray, &p.window);
                    p.on_theme_changed();
                }
            }
            TaoEvent::Reopen { .. } => {
                reopen(
                    &mut d,
                    now,
                    &mut bluetooth,
                    &panel,
                    &tray,
                    &mut ignore_blur_until,
                );
                changed = true;
            }
            TaoEvent::UserEvent(event) => {
                changed = true;
                match event {
                    Event::Connected(name, address) => {
                        eprintln!("Connected to {name}");
                        d.on_connected(name, address);
                        #[cfg(windows)]
                        if let Some(awareness) = &mut awareness {
                            awareness.control_connected(now);
                        }
                    }
                    Event::Data(bytes) => {
                        #[cfg(windows)]
                        if let Some(awareness) = &mut awareness {
                            awareness.receive(&mut d, &bytes, now);
                        }
                        #[cfg(not(windows))]
                        d.receive(&bytes);
                    }
                    Event::Disconnected => {
                        #[cfg(windows)]
                        if let Some(awareness) = &mut awareness {
                            awareness.control_closed(&d.address);
                        }
                        d.disconnect();
                    }
                    #[cfg(windows)]
                    Event::LinkSnapshot(snapshot) => {
                        changed = false;
                        if let Some(awareness) = &mut awareness {
                            awareness.link_snapshot(snapshot, &d, now);
                        }
                    }
                    #[cfg(windows)]
                    Event::BluetoothDevices(devices) => {
                        listed_devices = devices;
                        if tour_open && let Some(p) = &panel {
                            p.tour_devices(&listed_devices, &d, awareness.as_ref().is_some_and(|a| a.device_ready(&d)));
                        }
                    }
                    #[cfg(windows)]
                    Event::Media(generation, notice) => {
                        changed = false;
                        if let Some(awareness) = &mut awareness {
                            awareness.media(generation, notice, &d, now);
                        }
                    }
                    #[cfg(windows)]
                    Event::HardwareMedia(generation, command) => {
                        changed = false;
                        if let Some(awareness) = &awareness {
                            awareness.hardware_media(generation, command, &d);
                        }
                    }
                    #[cfg(windows)]
                    Event::TalkAudio(generation, result) => {
                        if let Some(awareness) = &mut awareness {
                            awareness.talk_audio(generation, result, now);
                        }
                    }
                    #[cfg(windows)]
                    Event::MediaDevicesChanged => {
                        changed = false;
                        if let Some(awareness) = &mut awareness {
                            awareness.media_devices_changed();
                        }
                    }
                    Event::Unrecognized(name) => d.set_unrecognized(name),
                    #[cfg(not(windows))]
                    Event::UpdateStatus(next) => {
                        update_notice_until = match &next {
                            UpdateStatus::UpToDate
                            | UpdateStatus::OpenedInstaller { .. }
                            | UpdateStatus::Error { manual: true, .. } => {
                                Some(now + Duration::from_millis(3500))
                            }
                            _ => None,
                        };
                        update_status = next;
                    }
                    #[cfg(not(windows))]
                    Event::UpdateRestart => quit = true,
                    #[cfg(windows)]
                    Event::WinXUpdate(message) => {
                        if let Some(version) = updates.message(message, now)
                            && let Some(awareness) = &mut awareness
                        {
                            awareness.update_notice(&version, now);
                        }
                    }
                    #[cfg(windows)]
                    Event::UpdateNoticeAction(action) => {
                        use overlay_windows::UpdateAction;
                        if let Some(awareness) = &mut awareness { awareness.dismiss_update_notice(now); }
                        match action {
                            UpdateAction::View => updates.open_release(),
                            UpdateAction::Settings => {
                                settings_route = "updates";
                                show_settings(target, &proxy, &mut settings_window, awareness.as_ref(), settings_route);
                            }
                            UpdateAction::Dismiss => {}
                        }
                    }
                    #[cfg(target_os = "macos")]
                    Event::OutsideClick => {
                        changed = false;
                        if let Some(p) = &panel
                            && p.is_open()
                            && now >= ignore_blur_until
                        {
                            p.hide();
                            last_hidden = now;
                        }
                    }
                    Event::Reopen => {
                        #[cfg(windows)]
                        if let Some(awareness) = &mut awareness {
                            awareness.refresh(now);
                        }
                        reopen(
                            &mut d,
                            now,
                            &mut bluetooth,
                            &panel,
                            &tray,
                            &mut ignore_blur_until,
                        );
                    }
                    Event::Tray(TrayIconEvent::Enter { .. }) => {
                        if let (Some(tray), Some(p)) = (&tray, &panel) {
                            panel::update_tray_icon(tray, &p.window);
                        }
                    }
                    #[cfg(windows)]
                    Event::DisplayChanged => {
                        if let Some(awareness) = &mut awareness {
                            awareness.visual_settings_changed();
                        }
                        if let (Some(tray), Some(p)) = (&tray, &panel) {
                            panel::update_tray_icon(tray, &p.window);
                            p.on_theme_changed();
                            if p.is_open() {
                                p.display_changed(tray);
                            }
                        }
                    }
                    #[cfg(windows)]
                    Event::Resumed => {
                        if let Some(awareness) = &mut awareness {
                            awareness.suspend(false, d.battery_revision, now);
                        }
                        if let Some(transport) = &mut bluetooth {
                            transport.refresh();
                        }
                        d.action(Action::Refresh, now);
                    }
                    #[cfg(windows)]
                    Event::Suspended => {
                        if let Some(awareness) = &mut awareness {
                            awareness.stop_talk(now);
                            awareness.suspend(true, d.battery_revision, now);
                        }
                    }
                    #[cfg(windows)]
                    Event::Hotkey(id) => {
                        let tour_focused = tour_open && panel.as_ref().is_some_and(|p| p.has_focus());
                        let effect = awareness.as_mut().map(|a| a.hotkey(id, &mut d, now,
                            exit_after_talk.is_none() && !updates.restoring() && (!tour_focused || tour_demo_live)));
                        if tour_focused && matches!(effect, Some(awareness_windows::ShortcutEffect::TalkMode))
                            && let Some(p) = &panel
                        {
                            p.tour_shortcut();
                        }
                        if !tour_open && matches!(effect, Some(awareness_windows::ShortcutEffect::TogglePanel))
                            && let Some(p) = &panel
                        {
                            if p.is_open() {
                                p.hide();
                                last_hidden = now;
                            } else {
                                if let Some(awareness) = &mut awareness { awareness.refresh(now); }
                                reopen(
                                    &mut d,
                                    now,
                                    &mut bluetooth,
                                    &panel,
                                    &tray,
                                    &mut ignore_blur_until,
                                );
                            }
                        }
                    }
                    #[cfg(windows)]
                    Event::TrayMenu(id) => match id.as_str() {
                        "tray:open" => {
                            if let Some(awareness) = &mut awareness {
                                awareness.refresh(now);
                            }
                            reopen(
                                &mut d,
                                now,
                                &mut bluetooth,
                                &panel,
                                &tray,
                                &mut ignore_blur_until,
                            );
                        }
                        "tray:startup" => {
                            let enabled = !platform::starts_with_windows();
                            if let Err(error) = platform::set_starts_with_windows(enabled) {
                                platform::show_information(
                                    APP_NAME,
                                    &format!("Could not change Start with Windows.\n\n{error}"),
                                );
                            }
                            startup_item.set_checked(platform::starts_with_windows());
                        }
                        "tray:updates" => {
                            settings_route = "updates";
                            show_settings(target, &proxy, &mut settings_window, awareness.as_ref(), settings_route);
                        }
                        "tray:about" => platform::show_information(
                            "About WinX",
                            concat!(
                                "WinX ",
                                env!("CARGO_PKG_VERSION"),
                                "\n\nDeveloped and maintained by Threalcocoboy.\nIndependent and unofficial. No affiliation with Nothing Technology Limited.\n",
                                "Based on NothingX Desktop by Nipun Yatawara and contributors. Licensed under AGPL-3.0."
                            ),
                        ),
                        "tray:quit" => quit = true,
                        _ => changed = false,
                    },
                    #[cfg(target_os = "macos")]
                    Event::Log(message) => {
                        eprintln!("{message}");
                        d.message = message;
                    }
                    #[cfg(target_os = "macos")]
                    Event::TransportExited => {
                        bluetooth.take();
                        d.disconnect();
                        restart = Some(now + Duration::from_secs(5));
                    }
                    Event::Tray(TrayIconEvent::Click {
                        button,
                        button_state: MouseButtonState::Up,
                        ..
                    }) => {
                        if let (Some(p), Some(tray)) = (&panel, &tray)
                            && button == MouseButton::Left
                        {
                            if p.is_open() {
                                p.hide();
                                last_hidden = now;
                            } else if now.duration_since(last_hidden) > Duration::from_millis(250) {
                                #[cfg(windows)]
                                if let Some(awareness) = &mut awareness {
                                    awareness.refresh(now);
                                }
                                d.action(Action::Refresh, now);
                                if !d.connected
                                    && let Some(t) = &mut bluetooth
                                {
                                    t.refresh();
                                }
                                ignore_blur_until = now + Duration::from_millis(250);
                                p.show(tray);
                            }
                        }
                    }
                    #[cfg(windows)]
                    Event::SettingsIpc(body) => {
                        if let (Some(window), Some(awareness), Ok(message)) =
                            (&settings_window, &mut awareness, serde_json::from_str::<Json>(&body))
                        {
                            let command = message["command"].as_str().unwrap_or("");
                            let args = message["args"].as_array().map(Vec::as_slice).unwrap_or(&[]);
                            let value = match command {
                                "init" => {
                                    let mut init = ui_state::init_with_update(&d, &update_status);
                                    init["version"] = json!(env!("CARGO_PKG_VERSION"));
                                    init["accent"] = json!(panel::accent());
                                    init["startup"] = json!(platform::starts_with_windows());
                                    init["winxUpdate"] = updates.snapshot();
                                    init["settingsRoute"] = json!(settings_route);
                                    init
                                }
                                "updateSettings" => {
                                    if let (Some(key), Some(enabled)) = (args.first().and_then(Json::as_str), args.get(1).and_then(Json::as_bool)) {
                                        updates.preference(key, enabled, now);
                                    }
                                    updates.snapshot()
                                }
                                "updateCheck" => { updates.check(&proxy); updates.snapshot() }
                                "updateDownload" => { updates.download(&proxy); updates.snapshot() }
                                "updateCancel" => { updates.dismiss_install(now); updates.snapshot() }
                                "updateRelease" => { updates.open_release(); Json::Null }
                                "updateConfirm" if args.first() == Some(&Json::Bool(true)) && exit_after_talk.is_none() => {
                                    updates.confirm(now);
                                    updates.snapshot()
                                }
                                "awarenessSettings" => awareness.settings(),
                                "setAwarenessSettings" => {
                                    let result = awareness.settings_command(args, now);
                                    if args.first().and_then(Json::as_str) == Some("theme") {
                                        apply_settings_theme(window, &result["settings"]["theme"]);
                                    }
                                    if args.first().and_then(Json::as_str) == Some("accentColor") {
                                        window.update_accent();
                                        if let Some(p) = &panel { p.update_accent(); }
                                    }
                                    result
                                },
                                "talkToggle" if exit_after_talk.is_none() && !updates.restoring() => {
                                    awareness.talk_toggle(&mut d, now); awareness.talk_status()
                                }
                                "retryRecovery" if exit_after_talk.is_none() && !updates.restoring() => {
                                    awareness.retry_talk(&d, now); awareness.talk_status()
                                }
                                "archiveRecovery" if args.first() == Some(&Json::Bool(true)) && !updates.restoring() =>
                                    json!({"error": awareness.archive_talk(now).err().map(|error| error.to_string()), "status": awareness.talk_status()}),
                                "settingsStartup" => {
                                    let error = args.first().and_then(Json::as_bool)
                                        .and_then(|enabled| platform::set_starts_with_windows(enabled).err());
                                    startup_item.set_checked(platform::starts_with_windows());
                                    json!({"startup": platform::starts_with_windows(), "error": error.map(|error| error.to_string())})
                                }
                                "settingsRoute" => {
                                    match args.first().and_then(Json::as_str) {
                                        Some(route @ ("replay" | "quick" | "equalizer" | "controls")) => {
                                            if let (Some(p), Some(tray)) = (&panel, &tray) {
                                                p.show(tray); p.route_panel(route);
                                            }
                                        }
                                        Some("pair") => platform::open_url("ms-settings:bluetooth"),
                                        Some("refresh") => {
                                            if let Some(t) = &mut bluetooth { t.refresh(); }
                                            d.action(Action::Refresh, now);
                                        }
                                        Some("upstream") => platform::open_url("https://github.com/nipunyatawara-dev/NothingX-Desktop"),
                                        Some("earlier") => platform::open_url("https://github.com/yashraut362/nothing-x-mac"),
                                        Some("protocol") => platform::open_url("https://github.com/radiance-project/ear-web"),
                                        Some("license") => platform::open_url("https://www.gnu.org/licenses/agpl-3.0.html"),
                                        _ => {}
                                    }
                                    Json::Null
                                }
                                "exportDiagnostics" => {
                                    let dir = platform::data_dir();
                                    let path = dir.join("winx-diagnostics.json");
                                    let media = std::fs::metadata(dir.join("winx-media-diagnostics.log")).ok()
                                        .filter(|info| info.len() <= 135_000)
                                        .and_then(|_| std::fs::read_to_string(dir.join("winx-media-diagnostics.log")).ok());
                                    let report = json!({"version": env!("CARGO_PKG_VERSION"), "pairedCandidates": listed_devices.len(),
                                        "controlConnected": d.connected, "verifiedDevice": awareness.device_ready(&d),
                                        "talk": awareness.talk_status(), "media": media});
                                    match std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()) {
                                        Ok(()) => json!({"file": path}),
                                        Err(error) => json!({"error": error.to_string()}),
                                    }
                                }
                                "uiError" => { eprintln!("Settings error: {:?}", args.first()); Json::Null }
                                _ => Json::Null,
                            };
                            window.reply(message["id"].as_u64().unwrap_or(0), &value);
                        }
                    }
                    Event::Ipc(body) => {
                        if let Ok(message) = serde_json::from_str::<Json>(&body) {
                            let command = message["command"].as_str().unwrap_or("");
                            let id = message["id"].as_u64().unwrap_or(0);
                            let args = message["args"].as_array().map(Vec::as_slice).unwrap_or(&[]);
                            if let Some(p) = &panel {
                                match command {
                                    "init" => {
                                        let mut init =
                                            ui_state::init_with_update(&d, &update_status);
                                        init["accent"] = json!(panel::accent());
                                        #[cfg(windows)]
                                        {
                                            init["winxSettings"] = Json::Bool(true);
                                            init["startOnboarding"] = json!(launch_tour
                                                && nothingx_desktop::preferences::Preferences::should_onboard(&platform::data_dir()));
                                            init["talkStatus"] = awareness.as_ref().map(|a| a.talk_status()).unwrap_or(Json::Null);
                                        }
                                        p.reply(id, &init);
                                        ready = true;
                                        #[cfg(windows)]
                                        update_ui_windows::acknowledge(&app_args);
                                        if auto_open && let Some(tray) = &tray {
                                            ignore_blur_until = now + Duration::from_millis(250);
                                            p.show(tray);
                                        }
                                    }
                                    #[cfg(windows)]
                                    "openSettings" | "updateAction" => {
                                        settings_route = if command == "updateAction" { "updates" } else { "general" };
                                        show_settings(target, &proxy, &mut settings_window, awareness.as_ref(), settings_route);
                                        if let Some(window) = &settings_window {
                                            window.state(&ui_state::snapshot_with_update(&d, &update_status));
                                        }
                                    }
                                    "motionPreference" => {
                                        if let Some(reduced) = args.first().and_then(Json::as_bool)
                                        {
                                            p.set_reduced_motion(reduced);
                                            #[cfg(windows)]
                                            if let Some(awareness) = &mut awareness {
                                                awareness.motion(reduced);
                                            }
                                        }
                                    }
                                    #[cfg(windows)]
                                    "awarenessSettings" => {
                                        if let Some(awareness) = &awareness {
                                            p.reply(id, &awareness.settings());
                                        }
                                    }
                                    #[cfg(windows)]
                                    "setAwarenessSettings" => {
                                        if let Some(awareness) = &mut awareness {
                                            p.reply(id, &awareness.settings_command(args, now));
                                        }
                                    }
                                    #[cfg(windows)]
                                    "talkToggle" => {
                                        if exit_after_talk.is_none() && !updates.restoring() && let Some(awareness) = &mut awareness {
                                            awareness.talk_toggle(&mut d, now);
                                            p.reply(id, &awareness.talk_status());
                                        }
                                    }
                                    #[cfg(windows)]
                                    "talkStop" => {
                                        if let Some(awareness) = &mut awareness {
                                            awareness.stop_talk(now);
                                        }
                                    }
                                    #[cfg(windows)]
                                    "onboarding" => {
                                        if let Some(awareness) = &mut awareness {
                                            let result = match args.first().and_then(Json::as_str) {
                                                Some("open") => {
                                                    tour_open = true;
                                                    tour_demo_live = false;
                                                    if let Some(tray) = &tray { p.open_tour(tray); }
                                                    p.tour_devices(&listed_devices, &d, awareness.device_ready(&d));
                                                    if let Some(t) = &mut bluetooth { t.refresh(); }
                                                    Ok(())
                                                }
                                                Some("demo") if tour_open => {
                                                    tour_demo_live = args.get(1).and_then(Json::as_bool).unwrap_or(false);
                                                    if !tour_demo_live { awareness.stop_talk(now); }
                                                    Ok(())
                                                }
                                                Some("rescan") if tour_open => {
                                                    if let Some(t) = &mut bluetooth { t.refresh(); }
                                                    d.action(Action::Refresh, now);
                                                    Ok(())
                                                }
                                                Some("pair") if tour_open => {
                                                    platform::open_url("ms-settings:bluetooth");
                                                    Ok(())
                                                }
                                                Some("select") if tour_open && awareness.talk_inactive() => {
                                                    let address = args.get(1).and_then(Json::as_u64);
                                                    if let Some(address) = address.filter(|address| *address == 0
                                                        || listed_devices.iter().any(|device| device.address == *address)
                                                        || u64::from_str_radix(&d.address, 16).ok() == Some(*address)) {
                                                        awareness.select_device(address).map(|()| {
                                                            if let Some(t) = &mut bluetooth { t.select(address); }
                                                        })
                                                    } else {
                                                        Err("That device is no longer in the list. Search again.")
                                                    }
                                                }
                                                Some("finish") if tour_open && awareness.talk_inactive() => {
                                                    awareness.complete_onboarding().map(|()| {
                                                        tour_open = false;
                                                        tour_demo_live = false;
                                                        p.close_tour();
                                                    })
                                                }
                                                Some("finish") if tour_open => Err("Finish restoring your listening settings first."),
                                                Some("leave") if tour_open => {
                                                    awareness.stop_talk(now);
                                                    tour_open = false;
                                                    tour_demo_live = false;
                                                    p.close_tour();
                                                    Ok(())
                                                }
                                                _ => Err("That onboarding action is unavailable."),
                                            };
                                            p.reply(id, &json!({"error": result.err()}));
                                        }
                                    }
                                    "setHeight" => {
                                        if let Some(h) = args.first().and_then(Json::as_f64) {
                                            p.resize(h);
                                        }
                                    }
                                    "quit" => quit = true,
                                    #[cfg(not(windows))]
                                    "updateAction" => {
                                        if let Some(release) =
                                            update_status.available_release().cloned()
                                        {
                                            update_notice_until = None;
                                            update_status = UpdateStatus::Downloading {
                                                version: release.version.clone(),
                                            };
                                            spawn_update_install(proxy.clone(), release);
                                        } else if !update_status.is_busy() {
                                            startup_update_check = None;
                                            update_notice_until = None;
                                            update_status = UpdateStatus::Checking { manual: true };
                                            spawn_update_check(proxy.clone(), true);
                                        } else {
                                            changed = false;
                                        }
                                    }
                                    "openExternal" => {
                                        changed = false;
                                        match args.first().and_then(Json::as_str) {
                                            Some("earWeb") => {
                                                platform::open_url(ui_state::EAR_WEB_URL);
                                                p.hide();
                                                last_hidden = now;
                                            }
                                            Some("reportIssue") => {
                                                platform::open_url(&ui_state::issue_url(&d));
                                                p.hide();
                                                last_hidden = now;
                                            }
                                            _ => {}
                                        }
                                    }
                                    "windowBlur" => {
                                        changed = false;
                                        if dismiss_on_blur && p.is_open() && !p.has_focus() && now >= ignore_blur_until
                                        {
                                            p.hide();
                                            last_hidden = now;
                                        }
                                    }
                                    "popupMenu" => {
                                        let menu = Menu::new();
                                        let items = args.first().and_then(Json::as_array);
                                        if let Some(items) = items.filter(|items| items.len() <= 32)
                                        {
                                            *popup_selection.lock().unwrap() = None;
                                            for (i, item) in items.iter().enumerate() {
                                                if let Some(label) = item["label"].as_str() {
                                                    let _ = menu.append(&CheckMenuItem::with_id(
                                                        format!("popup:{i}"),
                                                        label,
                                                        true,
                                                        item["checked"].as_bool().unwrap_or(false),
                                                        None,
                                                    ));
                                                }
                                            }
                                            let x =
                                                args.get(1).and_then(Json::as_f64).unwrap_or(0.);
                                            let y =
                                                args.get(2).and_then(Json::as_f64).unwrap_or(0.);
                                            ignore_blur_until =
                                                Instant::now() + Duration::from_secs(60);
                                            p.popup(&menu, Some((x, y)));
                                            ignore_blur_until =
                                                Instant::now() + Duration::from_millis(250);
                                            let _ = p.web.focus();
                                            let chosen = popup_selection
                                                .lock()
                                                .unwrap()
                                                .take()
                                                .and_then(|s| {
                                                    s.strip_prefix("popup:")?.parse::<usize>().ok()
                                                })
                                                .and_then(|i| items.get(i))
                                                .map(|v| v["value"].clone())
                                                .unwrap_or(Json::Null);
                                            p.reply(id, &chosen);
                                        } else {
                                            p.reply(id, &Json::Null);
                                        }
                                    }
                                    "uiError" => eprintln!("Panel error: {:?}", args.first()),
                                    _ => {
                                        if let Some(action) = ui_state::command(&d, command, args) {
                                            #[cfg(windows)]
                                            if let Some(awareness) = &mut awareness {
                                                awareness.apply_action(
                                                    &mut d,
                                                    action,
                                                    nothingx_desktop::awareness::Source::Panel,
                                                    now,
                                                );
                                            }
                                            #[cfg(not(windows))]
                                            d.action(action, now);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            TaoEvent::LoopDestroyed => {
                bluetooth.take();
                tray.take();
                return;
            }
            _ => {}
        }
        #[cfg(windows)]
        if let Some(awareness) = &mut awareness && updates.restoring() {
            let safe = awareness.safe_to_update();
            let transitioning = awareness.talk_transitioning();
            if updates.should_retry_talk(now, safe, transitioning) {
                awareness.retry_talk(&d, now);
            } else if updates.finish_restore(now, safe, transitioning, || awareness.save_for_update()) {
                quit = true;
            }
        }
        #[cfg(windows)]
        if (quit || quit_at.is_some_and(|at| now >= at)) && exit_after_talk.is_none() {
            if let Some(awareness) = &mut awareness {
                awareness.stop_talk(now);
            }
            exit_after_talk = Some(now + Duration::from_secs(10));
        }
        #[cfg(windows)]
        let quit = exit_after_talk.is_some_and(|deadline| now >= deadline
            || awareness.as_ref().is_none_or(|awareness| !awareness.talk_transitioning()));
        #[cfg(not(windows))]
        let quit = quit || quit_at.is_some_and(|at| now >= at);
        if quit {
            #[cfg(windows)]
            awareness.take();
            d.stop_ring();
            for bytes in d.tick(now + Duration::from_secs(1)) {
                if let Some(t) = &mut bluetooth {
                    let _ = t.write(&bytes);
                }
            }
            bluetooth.take();
            tray.take();
            *flow = ControlFlow::Exit;
            return;
        }
        if restart.is_some_and(|at| now >= at) {
            restart = None;
            match start_transport(proxy.clone(),
                #[cfg(windows)]
                awareness.as_ref().map_or(0, |a| a.preferred_device()),
            ) {
                Ok(t) => bluetooth = Some(t),
                Err(e) => {
                    d.message = e;
                    restart = Some(now + Duration::from_secs(10));
                }
            }
        }
        #[cfg(windows)]
        {
            updates.tick(now, &proxy);
            let snapshot = updates.snapshot();
            if snapshot != update_signature {
                update_signature = snapshot;
                if let Some(window) = &settings_window {
                    window.update_state(&update_signature);
                }
            }
        }
        #[cfg(not(windows))]
        if startup_update_check.is_some_and(|at| now >= at) {
            startup_update_check = None;
            if matches!(update_status, UpdateStatus::Idle) {
                update_status = UpdateStatus::Checking { manual: false };
                spawn_update_check(proxy.clone(), false);
            }
        }
        #[cfg(not(windows))]
        if update_notice_until.is_some_and(|at| now >= at) {
            update_notice_until = None;
            if matches!(
                update_status,
                UpdateStatus::UpToDate
                    | UpdateStatus::OpenedInstaller { .. }
                    | UpdateStatus::Error { .. }
            ) {
                update_status = UpdateStatus::Idle;
                changed = true;
            }
        }
        let due = d.deadline().is_some_and(|at| now >= at);
        changed |= due;
        for bytes in d.tick(now) {
            if let Some(t) = &mut bluetooth
                && let Err(e) = t.write(&bytes)
            {
                d.message = e;
            }
        }
        let mut status_dirty = false;
        if changed {
            let state = ui_state::snapshot_with_update(&d, &update_status);
            if state != signature {
                signature = state;
                status_dirty = true;
                if ready && let Some(p) = &panel {
                    p.state(&signature);
                    #[cfg(windows)]
                    if let Some(window) = &settings_window { window.state(&signature); }
                    #[cfg(windows)]
                    if tour_open { p.tour_devices(&listed_devices, &d, awareness.as_ref().is_some_and(|a| a.device_ready(&d))); }
                }
                if let Some(tray) = &tray {
                    let _ = tray.set_tooltip(Some(if d.connected {
                        format!("{APP_NAME} · {}", d.name)
                    } else {
                        APP_NAME.into()
                    }));
                }
            }
        }
        if let Some(p) = &panel {
            p.tick(now);
        }
        #[cfg(windows)]
        if let Some(awareness) = &mut awareness {
            awareness.tick(&mut d, now);
        }
        #[cfg(windows)]
        if let (Some(awareness), Some(p)) = (&awareness, &panel) {
            let state = awareness.talk_status();
            if ready && state != talk_signature {
                p.talk_state(&state);
                if let Some(window) = &settings_window { window.talk_state(&state); }
                talk_signature = state;
            }
        }
        #[cfg(windows)]
        let awareness_deadline = awareness
            .as_ref()
            .and_then(|service| service.deadline(&d, now));
        #[cfg(not(windows))]
        let awareness_deadline: Option<Instant> = None;
        let visibility = panel
            .as_ref()
            .map(|p| (p.window.is_visible(), p.is_open()))
            .unwrap_or_default();
        if visibility != previous_visibility {
            previous_visibility = visibility;
            status_dirty = true;
        }
        if ready != previous_ready {
            previous_ready = ready;
            status_dirty = true;
        }
        if status_dirty && let Some(path) = &status_path {
            if signature.is_null() {
                write_status(
                    path,
                    &ui_state::snapshot_with_update(&d, &update_status),
                    &d,
                    visibility,
                    ready,
                );
            } else {
                write_status(path, &signature, &d, visibility, ready);
            }
        }
        *flow = d
            .deadline()
            .into_iter()
            .chain(restart)
            .chain({
                #[cfg(windows)]
                { updates.deadline().into_iter().chain(None) }
                #[cfg(not(windows))]
                { startup_update_check.into_iter().chain(update_notice_until) }
            })
            .chain({
                #[cfg(windows)]
                { exit_after_talk.or(quit_at) }
                #[cfg(not(windows))]
                { quit_at }
            })
            .chain(panel.as_ref().and_then(|p| p.deadline()))
            .chain(awareness_deadline)
            .min()
            .map_or(ControlFlow::Wait, ControlFlow::WaitUntil);
    });
}
