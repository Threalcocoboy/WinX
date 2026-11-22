use nothingx_desktop::{
    awareness::{Kind, Notice, NoticeAction},
    preferences::{Appearance, OverlayOptions, Position, Size},
};
use std::{
    ffi::c_void,
    ptr::{null, null_mut},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
        Shell::{
            QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_QUIET_TIME, QUNS_RUNNING_D3D_FULL_SCREEN,
            SHQueryUserNotificationState,
        },
        WindowsAndMessaging::*,
    },
};

use crate::Event;
use crate::overlay_theme::Rgba;
use tao::event_loop::EventLoopProxy;
const FRAME: Duration = Duration::from_millis(16);

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

#[derive(Clone)]
struct Monitor {
    handle: HMONITOR,
    id: String,
    work: RECT,
    screen: RECT,
    primary: bool,
}

fn monitor_list() -> Vec<Monitor> {
    unsafe extern "system" fn add(
        handle: HMONITOR,
        _: HDC,
        _: *mut RECT,
        data: LPARAM,
    ) -> windows_sys::core::BOOL {
        let list = unsafe { &mut *(data as *mut Vec<Monitor>) };
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        if unsafe { GetMonitorInfoW(handle, (&mut info as *mut MONITORINFOEXW).cast()) } != 0 {
            let length = info
                .szDevice
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(info.szDevice.len());
            list.push(Monitor {
                handle,
                id: String::from_utf16_lossy(&info.szDevice[..length]),
                work: info.monitorInfo.rcWork,
                screen: info.monitorInfo.rcMonitor,
                primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
            });
        }
        1
    }
    let mut list: Vec<Monitor> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            null_mut(),
            null(),
            Some(add),
            (&mut list as *mut Vec<Monitor>) as LPARAM,
        );
    }
    list.sort_by_key(|m| (!m.primary, m.work.left, m.work.top));
    list
}

pub fn monitors() -> serde_json::Value {
    serde_json::Value::Array(
        monitor_list()
            .iter()
            .enumerate()
            .map(|(i, m)| {
                serde_json::json!({"id": m.id, "label": format!("Display {} ({} × {})", i + 1,
            m.screen.right - m.screen.left, m.screen.bottom - m.screen.top)})
            })
            .collect(),
    )
}

fn select_monitor(id: &str) -> Option<(Monitor, u32)> {
    let list = monitor_list();
    let foreground = unsafe { MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTOPRIMARY) };
    let monitor = list
        .iter()
        .find(|m| match id {
            "foreground" => m.handle == foreground,
            "primary" => m.primary,
            _ => m.id == id,
        })
        .or_else(|| list.iter().find(|m| m.primary))
        .or_else(|| list.first())?;
    let (mut dpi, mut vertical) = (96, 96);
    unsafe {
        GetDpiForMonitor(monitor.handle, MDT_EFFECTIVE_DPI, &mut dpi, &mut vertical);
    }
    Some((monitor.clone(), dpi.clamp(72, 480)))
}

fn location(work: RECT, size: SIZE, position: Position, margin: i32) -> POINT {
    let x = match position {
        Position::TopLeft | Position::BottomLeft => work.left + margin,
        Position::TopCenter | Position::BottomCenter => {
            work.left + (work.right - work.left - size.cx) / 2
        }
        Position::TopRight | Position::BottomRight => work.right - size.cx - margin,
    };
    let y = match position {
        Position::TopLeft | Position::TopCenter | Position::TopRight => work.top + margin,
        _ => work.bottom - size.cy - margin,
    };
    POINT {
        x: x.clamp(work.left, (work.right - size.cx).max(work.left)),
        y: y.clamp(work.top, (work.bottom - size.cy).max(work.top)),
    }
}

pub fn quiet(respect_quiet: bool) -> bool {
    let mut state = 0;
    if unsafe { SHQueryUserNotificationState(&mut state) } >= 0
        && (matches!(state, QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE)
            || respect_quiet && matches!(state, QUNS_BUSY | QUNS_QUIET_TIME))
    {
        return true;
    }
    let window = unsafe { GetForegroundWindow() };
    let style = unsafe { GetWindowLongPtrW(window, GWL_STYLE) } as u32;
    if style & WS_POPUP == 0 || style & WS_CAPTION != 0 {
        return false;
    }
    let mut bounds = RECT::default();
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        let monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONULL);
        GetWindowRect(window, &mut bounds) != 0
            && GetMonitorInfoW(monitor, &mut info) != 0
            && bounds.left <= info.rcMonitor.left
            && bounds.top <= info.rcMonitor.top
            && bounds.right >= info.rcMonitor.right
            && bounds.bottom >= info.rcMonitor.bottom
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let interactive =
        unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32 & WS_EX_TRANSPARENT == 0;
    match msg {
        WM_NCHITTEST if !interactive => HTTRANSPARENT as LRESULT,
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_LBUTTONUP if interactive => {
            let mut bounds = RECT::default();
            let proxy =
                unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const EventLoopProxy<Event>;
            if !proxy.is_null()
                && unsafe { GetClientRect(hwnd, &mut bounds) } != 0
                && let Some(action) =
                    update_action(l as i16 as i32, (l >> 16) as i16 as i32, &bounds)
            {
                let _ = unsafe { &*proxy }.send_event(Event::UpdateNoticeAction(action));
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

const WIDTH: f64 = 280.;
const HEIGHT: f64 = 54.;
const UPDATE_HEIGHT: f64 = 84.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateAction {
    View,
    Settings,
    Dismiss,
}

const UPDATE_ACTIONS: [(UpdateAction, &str); 3] = [
    (UpdateAction::View, "View update"),
    (UpdateAction::Settings, "Settings"),
    (UpdateAction::Dismiss, "Dismiss"),
];

fn update_action(x: i32, y: i32, bounds: &RECT) -> Option<UpdateAction> {
    let width = bounds.right;
    let height = bounds.bottom;
    if x < 0 || x >= width || y < height * 54 / 84 || y >= height * 78 / 84 {
        return None;
    }
    Some(UPDATE_ACTIONS[(x * 3 / width) as usize].0)
}

struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut u32,
    size: SIZE,
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

fn rounded_distance(x: f64, y: f64, width: f64, height: f64, radius: f64) -> f64 {
    let x = (x - width / 2.).abs() - (width / 2. - radius);
    let y = (y - height / 2.).abs() - (height / 2. - radius);
    x.max(0.).hypot(y.max(0.)) + x.max(y).min(0.) - radius
}

fn over(pixel: &mut u32, color: Rgba, cover: f64) {
    let alpha = (color.3 * cover * 255.).round().clamp(0., 255.) as u32;
    let blend =
        |channel: u32, previous: u32| (channel * alpha + previous * (255 - alpha) + 127) / 255;
    let old = *pixel;
    *pixel = (blend(255, old >> 24) << 24)
        | (blend(u32::from(color.0), (old >> 16) & 255) << 16)
        | (blend(u32::from(color.1), (old >> 8) & 255) << 8)
        | blend(u32::from(color.2), old & 255);
}

fn draw_text(dc: HDC, value: &str, mut bounds: RECT, size: i32, weight: i32, family: &str) {
    let value = wide(value);
    let font_for = |name| unsafe {
        CreateFontW(
            -size,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            0,
            0,
            ANTIALIASED_QUALITY as u32,
            0,
            wide(name).as_ptr(),
        )
    };
    let mut font = font_for(family);
    if font.is_null() {
        return;
    }
    let old = unsafe { SelectObject(dc, font) };
    let mut actual = [0u16; 64];
    unsafe {
        GetTextFaceW(dc, actual.len() as i32, actual.as_mut_ptr());
    }
    let len = actual
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(actual.len());
    if !String::from_utf16_lossy(&actual[..len]).starts_with("Segoe UI") {
        let fallback = font_for("Segoe UI");
        if !fallback.is_null() {
            unsafe {
                SelectObject(dc, fallback);
                DeleteObject(font);
            }
            font = fallback;
        }
    }
    unsafe {
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, 0x00ff_ffff);
        DrawTextW(
            dc,
            value.as_ptr(),
            -1,
            &mut bounds,
            DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS,
        );
        SelectObject(dc, old);
        DeleteObject(font);
        GdiFlush();
    }
}

impl Surface {
    fn blank(size: SIZE) -> Option<Self> {
        if size.cx < 1 || size.cy < 1 {
            return None;
        }
        let mut bitmap = BITMAPINFO::default();
        bitmap.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        bitmap.bmiHeader.biWidth = size.cx;
        bitmap.bmiHeader.biHeight = -size.cy;
        bitmap.bmiHeader.biPlanes = 1;
        bitmap.bmiHeader.biBitCount = 32;
        let dc = unsafe { CreateCompatibleDC(null_mut()) };
        if dc.is_null() {
            return None;
        }
        let mut bits: *mut c_void = null_mut();
        let image =
            unsafe { CreateDIBSection(dc, &bitmap, DIB_RGB_COLORS, &mut bits, null_mut(), 0) };
        if image.is_null() || bits.is_null() {
            unsafe {
                DeleteDC(dc);
            }
            return None;
        }
        let previous = unsafe { SelectObject(dc, image) };
        Some(Self {
            dc,
            bitmap: image,
            previous,
            bits: bits.cast(),
            size,
        })
    }

    fn pixels(&self) -> &[u32] {
        unsafe { std::slice::from_raw_parts(self.bits, (self.size.cx * self.size.cy) as usize) }
    }
    fn pixels_mut(&mut self) -> &mut [u32] {
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.size.cx * self.size.cy) as usize) }
    }
    fn index(&self, x: i32, y: i32) -> usize {
        (y * self.size.cx + x) as usize
    }

    fn lerp(&mut self, from: &Self, to: &Self, alpha: f64) {
        let weight = (alpha * 255.).round() as u32;
        for ((pixel, &a), &b) in self
            .pixels_mut()
            .iter_mut()
            .zip(from.pixels())
            .zip(to.pixels())
        {
            let mix = |shift| {
                let (a, b): (u32, u32) = ((a >> shift) & 255u32, (b >> shift) & 255u32);
                ((a * (255 - weight) + b * weight + 127) / 255) << shift
            };
            *pixel = mix(24) | mix(16) | mix(8) | mix(0);
        }
    }

    fn new(notice: &Notice, options: &OverlayOptions, monitor: &Monitor, dpi: u32) -> Option<Self> {
        let interactive = notice.action == NoticeAction::UpdateAvailable;
        let height = if interactive { UPDATE_HEIGHT } else { HEIGHT };
        let base = f64::from(dpi) / 96.;
        let factor = match options.size {
            Size::Compact => 0.86,
            Size::Normal => 1.,
            Size::Large => 1.18,
        };
        let scale = (base * factor)
            .min(f64::from(monitor.work.right - monitor.work.left) / (WIDTH + 12.))
            .min(f64::from(monitor.work.bottom - monitor.work.top) / (height + 12.));
        if scale < 0.35 {
            return None;
        }
        let px = |n: f64| (n * scale).round() as i32;
        let size = SIZE {
            cx: px(WIDTH),
            cy: px(height),
        };
        let mut result = Self::blank(size)?;
        let mut mask = Self::blank(size)?;
        let dark = match options.appearance {
            Appearance::Dark => true,
            Appearance::Light => false,
            Appearance::System => crate::panel::windows_apps_dark(),
        };
        let tokens = crate::overlay_theme::design(dark);
        let inset = px(7.);
        let top = px(6.);
        let card_w = f64::from(size.cx - 2 * inset);
        let card_h = f64::from(px(tokens.badge
            + 2. * tokens.padding
            + if interactive { 30. } else { 0. }));
        let radius = tokens.radius * scale;
        let badge_size = px(tokens.badge);
        let badge_x = inset + px(tokens.inset);
        let badge_y = top + px(tokens.padding);
        let pixels = result.pixels_mut();
        for y in 0..size.cy {
            for x in 0..size.cx {
                let (a, b) = (f64::from(x - inset) + 0.5, f64::from(y - top) + 0.5);
                let distance = rounded_distance(a, b, card_w, card_h, radius);
                let edge = (0.5 - distance).clamp(0., 1.);
                let shadow = (1.
                    - rounded_distance(a, b - scale, card_w, card_h, radius).max(0.)
                        / (scale * 4.))
                    .clamp(0., 1.);
                let pixel = &mut pixels[(y * size.cx + x) as usize];
                *pixel = 0;
                over(pixel, Rgba(0, 0, 0, tokens.shadow), shadow * (1. - edge));
                over(pixel, tokens.material, edge);
                over(pixel, tokens.fill, edge);
                over(
                    pixel,
                    tokens.border,
                    (edge - (0.5 - tokens.stroke * scale - distance).clamp(0., 1.)).max(0.),
                );
                let badge = rounded_distance(
                    f64::from(x - badge_x) + 0.5,
                    f64::from(y - badge_y) + 0.5,
                    f64::from(badge_size),
                    f64::from(badge_size),
                    tokens.badge_radius * scale,
                );
                over(pixel, tokens.chip, (0.5 - badge).clamp(0., 1.));
            }
        }
        let icon_size = px(tokens.icon);
        let icon = crate::overlay_icons::paint(notice.action, icon_size as usize);
        let inset_in_badge = (badge_size - icon_size) / 2;
        let accent = Rgba::hex(&crate::panel::accent()).unwrap_or(tokens.accent);
        let mark = if notice.kind == Kind::Battery {
            tokens.warning
        } else {
            accent
        };
        for y in 0..icon_size {
            for x in 0..icon_size {
                let index =
                    result.index(badge_x + inset_in_badge + x, badge_y + inset_in_badge + y);
                let pixel = &mut result.pixels_mut()[index];
                let ink = (y * icon_size + x) as usize;
                over(pixel, tokens.text, f64::from(icon.ink[ink]) / 255.);
                over(pixel, mark, f64::from(icon.accent[ink]) / 255.);
            }
        }
        let left = badge_x + badge_size + px(tokens.gap);
        let right = size.cx - inset - px(tokens.inset);
        let caption = !notice.detail.is_empty();
        let split = badge_y + px(15.);
        let (title_end, line_end) = if caption {
            (split + px(1.), split + px(14.))
        } else {
            (badge_y + badge_size, badge_y + badge_size)
        };
        for (label, low, high, size, weight, color) in [
            (
                notice.title.as_str(),
                badge_y,
                title_end,
                tokens.title,
                tokens.weight,
                tokens.text,
            ),
            (
                notice.detail.as_str(),
                split,
                line_end,
                tokens.subtitle,
                FW_NORMAL as i32,
                tokens.secondary,
            ),
        ] {
            if label.is_empty() {
                continue;
            }
            mask.pixels_mut().fill(0);
            draw_text(
                mask.dc,
                label,
                RECT {
                    left,
                    top: low,
                    right,
                    bottom: high,
                },
                px(size),
                weight,
                tokens.font,
            );
            for (target, &coverage) in result.pixels_mut().iter_mut().zip(mask.pixels()) {
                if coverage != 0 {
                    over(target, color, f64::from(coverage & 255) / 255.);
                }
            }
        }
        if interactive {
            for (index, (_, label)) in UPDATE_ACTIONS.iter().enumerate() {
                let column = size.cx / 3;
                mask.pixels_mut().fill(0);
                draw_text(
                    mask.dc,
                    label,
                    RECT {
                        left: index as i32 * column + px(16.),
                        right: (index as i32 + 1) * column - px(4.),
                        top: px(54.),
                        bottom: px(78.),
                    },
                    px(tokens.subtitle),
                    tokens.weight,
                    tokens.font,
                );
                for (target, &coverage) in result.pixels_mut().iter_mut().zip(mask.pixels()) {
                    if coverage != 0 {
                        over(target, tokens.text, f64::from(coverage & 255) / 255.);
                    }
                }
            }
        }
        Some(result)
    }
}

#[derive(Clone, Copy)]
struct Fade {
    from: f64,
    to: f64,
    started: Instant,
}

impl Fade {
    fn at(self, now: Instant, duration: Duration, reduced: bool) -> (f64, bool) {
        let progress = if reduced {
            1.
        } else {
            (now.saturating_duration_since(self.started).as_secs_f64() / duration.as_secs_f64())
                .min(1.)
        };
        let ease = 1. - (1. - progress).powi(3);
        (
            self.from + (self.to - self.from) * ease,
            progress < 1. && self.from != self.to,
        )
    }
}

struct Crossfade {
    from: Surface,
    frame: Surface,
    started: Instant,
}

pub struct Overlay {
    hwnd: HWND,
    _proxy: Option<Box<EventLoopProxy<Event>>>,
    surface: Option<Surface>,
    notice: Option<Notice>,
    transition: Option<Crossfade>,
    destination: POINT,
    fade: Fade,
    moving: bool,
    reduced: bool,
    direction: i32,
    settings_revision: u64,
}

impl Overlay {
    pub fn new(now: Instant, proxy: Option<EventLoopProxy<Event>>) -> Self {
        let class = wide("WinXOverlayWindow");
        let instance = unsafe { GetModuleHandleW(null()) };
        let attributes = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..Default::default()
        };
        unsafe {
            RegisterClassW(&attributes);
        }
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE
                    | WS_EX_TOPMOST,
                class.as_ptr(),
                wide("WinX notification").as_ptr(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                instance,
                null(),
            )
        };
        let proxy = proxy.map(Box::new);
        if let Some(proxy) = proxy.as_deref()
            && !hwnd.is_null()
        {
            unsafe {
                SetWindowLongPtrW(
                    hwnd,
                    GWLP_USERDATA,
                    (proxy as *const EventLoopProxy<Event>) as isize,
                );
            }
        }
        Self {
            hwnd,
            _proxy: proxy,
            surface: None,
            notice: None,
            transition: None,
            destination: POINT::default(),
            fade: Fade {
                from: 0.,
                to: 0.,
                started: now,
            },
            moving: false,
            reduced: false,
            direction: 1,
            settings_revision: 0,
        }
    }

    pub fn set_reduced(&mut self, reduced: bool) {
        self.reduced = reduced;
    }

    pub fn render(
        &mut self,
        notice: Option<&Notice>,
        options: &OverlayOptions,
        revision: u64,
        now: Instant,
    ) {
        if self.hwnd.is_null() {
            return;
        }
        let skin = crate::overlay_theme::design(false);
        let changed = self.notice.as_ref() != notice || revision != self.settings_revision;
        if changed {
            let style = unsafe { GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) } as u32;
            let interactive =
                notice.is_some_and(|notice| notice.action == NoticeAction::UpdateAvailable);
            let style = if interactive {
                style & !WS_EX_TRANSPARENT
            } else {
                style | WS_EX_TRANSPARENT
            };
            unsafe {
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, style as isize);
            }
            let target = if notice.is_some() { 1. } else { 0. };
            if self.fade.to != target {
                self.fade = Fade {
                    from: self.fade.at(now, skin.animation, self.reduced).0,
                    to: target,
                    started: now,
                };
            }
            if let Some(notice) = notice {
                let Some((monitor, dpi)) = select_monitor(&options.monitor) else {
                    self.hide();
                    return;
                };
                let Some(surface) = Surface::new(notice, options, &monitor, dpi) else {
                    self.hide();
                    return;
                };
                let destination = location(
                    monitor.work,
                    surface.size,
                    options.position,
                    (f64::from(dpi) / 8.) as i32,
                );
                let previous = self
                    .transition
                    .take()
                    .map(|fade| fade.frame)
                    .or_else(|| self.surface.take());
                if !self.reduced
                    && revision == self.settings_revision
                    && self.destination.x == destination.x
                    && self.destination.y == destination.y
                    && let Some(from) = previous
                        .filter(|p| p.size.cx == surface.size.cx && p.size.cy == surface.size.cy)
                    && let Some(frame) = Surface::blank(surface.size)
                {
                    self.transition = Some(Crossfade {
                        from,
                        frame,
                        started: now,
                    });
                }
                self.destination = destination;
                self.direction = if matches!(
                    options.position,
                    Position::TopLeft | Position::TopCenter | Position::TopRight
                ) {
                    -1
                } else {
                    1
                };
                self.surface = Some(surface);
            }
            self.settings_revision = revision;
            self.notice = notice.cloned();
        }
        if !changed && !self.moving {
            return;
        }
        let (alpha, moving) = self.fade.at(now, skin.animation, self.reduced);
        if self.fade.to == 0. && alpha <= 0. {
            self.hide();
            return;
        }
        let Some(target) = &self.surface else {
            self.moving = false;
            return;
        };
        let mut changing_content = false;
        let surface = if let Some(transition) = &mut self.transition {
            let change = Fade {
                from: 0.,
                to: 1.,
                started: transition.started,
            };
            let (amount, active) = change.at(now, skin.animation, self.reduced);
            if active {
                changing_content = true;
                transition.frame.lerp(&transition.from, target, amount);
                &transition.frame
            } else {
                self.transition = None;
                target
            }
        } else {
            target
        };
        self.moving = moving || changing_content;
        let destination = POINT {
            x: self.destination.x,
            y: self.destination.y + self.direction * ((1. - alpha) * skin.slide).round() as i32,
        };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: (alpha * 255.) as u8,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        unsafe {
            if UpdateLayeredWindow(
                self.hwnd,
                null_mut(),
                &destination,
                &surface.size,
                surface.dc,
                &POINT::default(),
                0,
                &blend,
                ULW_ALPHA,
            ) != 0
                && IsWindowVisible(self.hwnd) == 0
            {
                ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
        }
    }

    pub fn hide(&mut self) {
        unsafe {
            ShowWindow(self.hwnd, SW_HIDE);
        }
        self.surface = None;
        self.notice = None;
        self.transition = None;
        self.fade.from = 0.;
        self.fade.to = 0.;
        self.moving = false;
    }

    pub fn deadline(&self, now: Instant) -> Option<Instant> {
        self.moving.then_some(now + FRAME)
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        if !self.hwnd.is_null() {
            unsafe {
                DestroyWindow(self.hwnd);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nothingx_desktop::awareness::Source;

    #[test]
    fn update_actions_follow_overlay_size() {
        for scale in [1, 2, 3] {
            let bounds = RECT {
                right: 280 * scale,
                bottom: 84 * scale,
                ..Default::default()
            };
            assert_eq!(
                update_action(30 * scale, 63 * scale, &bounds),
                Some(UpdateAction::View)
            );
            assert_eq!(
                update_action(140 * scale, 63 * scale, &bounds),
                Some(UpdateAction::Settings)
            );
            assert_eq!(
                update_action(240 * scale, 63 * scale, &bounds),
                Some(UpdateAction::Dismiss)
            );
            assert_eq!(update_action(140 * scale, 30 * scale, &bounds), None);
        }
    }

    #[test]
    fn positions_sizes_and_scales_fit_negative_coordinate_work_area() {
        for scale in [1., 1.25, 1.5, 1.75, 2.] {
            let work = RECT {
                left: -2560,
                top: 40,
                right: -20,
                bottom: 1400,
            };
            for factor in [0.86, 1., 1.18] {
                let size = SIZE {
                    cx: (WIDTH * scale * factor) as i32,
                    cy: (HEIGHT * scale * factor) as i32,
                };
                let margin = (12.0 * scale) as i32;
                for (row, column, position) in [
                    (0, 0, Position::TopLeft),
                    (0, 1, Position::TopCenter),
                    (0, 2, Position::TopRight),
                    (1, 0, Position::BottomLeft),
                    (1, 1, Position::BottomCenter),
                    (1, 2, Position::BottomRight),
                ] {
                    let p = location(work, size, position, margin);
                    let expected_x = [
                        work.left + margin,
                        (work.left + work.right - size.cx) / 2,
                        work.right - size.cx - margin,
                    ];
                    let expected_y = [work.top + margin, work.bottom - size.cy - margin];
                    assert!((p.x - expected_x[column]).abs() <= 1);
                    assert_eq!(p.y, expected_y[row]);
                }
            }
        }
    }

    #[test]
    fn native_window_is_hidden_nonactivating_click_through_and_destroyed() {
        let overlay = Overlay::new(Instant::now(), None);
        let hwnd = overlay.hwnd;
        assert!(!hwnd.is_null());
        let mask =
            WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_TOPMOST;
        assert_eq!(
            unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32 & mask,
            mask
        );
        assert_eq!(unsafe { IsWindowVisible(hwnd) }, 0);
        assert_eq!(
            unsafe { window_proc(hwnd, WM_NCHITTEST, 0, 0) },
            HTTRANSPARENT as isize
        );
        assert_eq!(
            unsafe { window_proc(hwnd, WM_MOUSEACTIVATE, 0, 0) },
            MA_NOACTIVATE as isize
        );
        drop(overlay);
        assert_eq!(unsafe { IsWindow(hwnd) }, 0);
    }

    #[test]
    fn different_notice_types_share_the_same_nonempty_layout_in_both_themes() {
        let work = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        let monitor = Monitor {
            handle: null_mut(),
            id: String::new(),
            work,
            screen: work,
            primary: true,
        };
        for (action, title, caption) in [
            (
                NoticeAction::Connected,
                "Nothing Ear (3)",
                "Left 84%  ·  Case 62%",
            ),
            (NoticeAction::Disconnected, "CMF Buds Pro 2", "Disconnected"),
            (
                NoticeAction::BatteryCritical,
                "Battery critical",
                "Right · 5% remaining",
            ),
            (NoticeAction::Play, "Playing", ""),
            (NoticeAction::Pause, "Paused", ""),
        ] {
            let notice = Notice::new(action, Source::Preview, title, caption);
            for appearance in [Appearance::Light, Appearance::Dark] {
                let options = OverlayOptions {
                    appearance,
                    ..OverlayOptions::default()
                };
                let surface = Surface::new(&notice, &options, &monitor, 144).unwrap();
                assert_eq!(surface.size.cx, 420);
                assert_eq!(surface.size.cy, 81);
                assert_eq!(surface.pixels()[0], 0);
                assert!(surface.pixels().iter().any(|&pixel| pixel >> 24 >= 220));
            }
        }
    }

    #[test]
    fn update_notice_renders_its_actions_at_multiple_dpi_and_in_both_themes() {
        let work = RECT {
            right: 1920,
            bottom: 1040,
            ..Default::default()
        };
        let monitor = Monitor {
            handle: null_mut(),
            id: String::new(),
            work,
            screen: work,
            primary: true,
        };
        let notice = Notice::new(
            NoticeAction::UpdateAvailable,
            Source::Preview,
            "WinX update available",
            "A new update is ready",
        );
        for appearance in [Appearance::Light, Appearance::Dark] {
            let options = OverlayOptions {
                appearance,
                ..Default::default()
            };
            for dpi in [96, 144, 192] {
                let surface = Surface::new(&notice, &options, &monitor, dpi).unwrap();
                assert_eq!(surface.size.cy, 84 * dpi as i32 / 96);
                for column in 0..3 {
                    let x = surface.size.cx * (column * 2 + 1) / 6;
                    let y = surface.size.cy * 62 / 84;
                    assert!(surface.pixels()[surface.index(x, y)] >> 24 > 0);
                }
                if matches!(appearance, Appearance::Dark)
                    && dpi == 192
                    && let Some(path) = std::env::var_os("WINX_OVERLAY_SAMPLE")
                {
                    let bytes: Vec<_> = surface
                        .pixels()
                        .iter()
                        .flat_map(|pixel| {
                            let alpha = pixel >> 24;
                            let channel = |shift: u32| {
                                ((pixel >> shift & 255) + 242 * (255 - alpha) / 255).min(255) as u8
                            };
                            [channel(16), channel(8), channel(0)]
                        })
                        .collect();
                    let output = std::fs::File::create(path).unwrap();
                    let mut encoder =
                        png::Encoder::new(output, surface.size.cx as u32, surface.size.cy as u32);
                    encoder.set_color(png::ColorType::Rgb);
                    encoder
                        .write_header()
                        .unwrap()
                        .write_image_data(&bytes)
                        .unwrap();
                }
            }
        }
    }

    #[test]
    fn crossfade_preserves_the_shared_backdrop_and_reduced_motion_finishes_immediately() {
        let size = SIZE { cx: 2, cy: 1 };
        let mut from = Surface::blank(size).unwrap();
        let mut to = Surface::blank(size).unwrap();
        let mut frame = Surface::blank(size).unwrap();
        from.pixels_mut().copy_from_slice(&[0xccaaaaaa, 0xff000000]);
        to.pixels_mut().copy_from_slice(&[0xccaaaaaa, 0xffffffff]);
        frame.lerp(&from, &to, 0.5);
        assert_eq!(frame.pixels(), &[0xccaaaaaa, 0xff808080]);
        let now = Instant::now();
        let duration = crate::overlay_theme::design(false).animation;
        let entrance = Fade {
            from: 0.,
            to: 1.,
            started: now,
        };
        let (current, active) = entrance.at(now + Duration::from_millis(40), duration, false);
        assert!(active && current > 0. && current < 1.);
        let reversal = Fade {
            from: current,
            to: 0.,
            started: now,
        };
        assert_eq!(reversal.at(now, duration, false).0, current);
        assert_eq!(reversal.at(now, duration, true), (0., false));
    }
}
