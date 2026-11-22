//! One icon catalog for live notifications and previews. Existing WinX
//! glyphs are used as monochrome masks. New marks are drawn as vectors in
//! the same 16 px coordinate system.
use base64::Engine;
use nothingx_desktop::awareness::NoticeAction;
use std::{cell::RefCell, collections::BTreeMap, io::Cursor, sync::OnceLock};

#[derive(Clone, Copy)]
enum Shape {
    Play,
    Pause,
    Next,
    Previous,
    Speaker,
    Battery,
    Download,
}

#[derive(Clone, Copy)]
enum Mark {
    None,
    Check,
    Slash,
    Plus,
    Minus,
    Up,
    Down,
    Cross,
    Waves,
    Low,
    Warning,
    Critical,
    Full,
    Unavailable,
    Locate,
}

#[derive(Clone, Copy)]
enum Base {
    Existing(&'static str),
    Vector(Shape),
}

fn artwork(action: NoticeAction) -> (Base, Mark) {
    use Base::*;
    use NoticeAction::*;
    match action {
        Connected => (Existing("budLeft"), Mark::Check),
        HeadphonesConnected => (Existing("headphones"), Mark::Check),
        Disconnected => (Existing("headphones"), Mark::Slash),
        AncOn => (Existing("anc"), Mark::None),
        AncHigh => (Existing("ancHigh"), Mark::None),
        AncMedium => (Existing("ancMid"), Mark::None),
        AncLow => (Existing("ancLow"), Mark::None),
        AncOff => (Existing("off"), Mark::None),
        Transparency => (Existing("transparency"), Mark::None),
        AdaptiveAnc => (Existing("ancAdaptive"), Mark::None),
        Play => (Vector(Shape::Play), Mark::None),
        Pause => (Vector(Shape::Pause), Mark::None),
        NextTrack => (Vector(Shape::Next), Mark::None),
        PreviousTrack => (Vector(Shape::Previous), Mark::None),
        VolumeUp => (Vector(Shape::Speaker), Mark::Plus),
        VolumeDown => (Vector(Shape::Speaker), Mark::Minus),
        Mute => (Vector(Shape::Speaker), Mark::Cross),
        Unmute => (Vector(Shape::Speaker), Mark::Waves),
        BatteryLow => (Vector(Shape::Battery), Mark::Low),
        BatteryVeryLow => (Vector(Shape::Battery), Mark::Warning),
        BatteryCritical => (Vector(Shape::Battery), Mark::Critical),
        BatteryStatus => (Vector(Shape::Battery), Mark::Full),
        BatteryUnavailable => (Vector(Shape::Battery), Mark::Unavailable),
        LatencyOn => (Existing("qsLatency"), Mark::Check),
        LatencyOff => (Existing("qsLatency"), Mark::Slash),
        BassUp => (Existing("bassWave"), Mark::Up),
        BassDown => (Existing("bassWave"), Mark::Down),
        BassOn => (Existing("bassToggle"), Mark::Check),
        BassOff => (Existing("bassToggle"), Mark::Slash),
        Equalizer => (Existing("equalizer"), Mark::None),
        CustomEqualizer => (Existing("eq_custom"), Mark::None),
        RingLeft => (Existing("budLeft"), Mark::Locate),
        RingRight => (Existing("budRight"), Mark::Locate),
        RingBoth => (Existing("qsFind"), Mark::Locate),
        RingStop => (Existing("qsFind"), Mark::Slash),
        Spatial => (Existing("qsSpatial"), Mark::None),
        InEarOn => (Existing("qsInEar"), Mark::Check),
        InEarOff => (Existing("qsInEar"), Mark::Slash),
        PersonalOn => (Existing("qsPersonalSound"), Mark::Check),
        PersonalOff => (Existing("qsPersonalSound"), Mark::Slash),
        DualOn => (Existing("qsDual"), Mark::Check),
        DualOff => (Existing("qsDual"), Mark::Slash),
        OverlayEnabled => (Existing("settings"), Mark::Check),
        TalkOn => (Existing("transparency"), Mark::Waves),
        TalkOff => (Existing("transparency"), Mark::Check),
        TalkPartial => (Existing("transparency"), Mark::Warning),
        UpdateAvailable => (Vector(Shape::Download), Mark::None),
    }
}

struct Mask {
    width: usize,
    height: usize,
    alpha: Vec<u8>,
}

fn decode(key: &str) -> Option<Mask> {
    static ASSETS: OnceLock<serde_json::Value> = OnceLock::new();
    let assets = ASSETS.get_or_init(nothingx_desktop::ui_state::appearance);
    let url = assets["symbols"][key]
        .as_str()?
        .strip_prefix("data:image/png;base64,")?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(url).ok()?;
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut rgba = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut rgba).ok()?;
    let (channels, alpha_index) = match info.color_type {
        png::ColorType::Rgba => (4, 3),
        png::ColorType::GrayscaleAlpha => (2, 1),
        _ => return None,
    };
    let (width, height) = (info.width as usize, info.height as usize);
    let mut bounds = (width, height, 0, 0);
    for (i, pixel) in rgba[..info.buffer_size()]
        .chunks_exact(channels)
        .enumerate()
    {
        if pixel[alpha_index] > 12 {
            bounds.0 = bounds.0.min(i % width);
            bounds.1 = bounds.1.min(i / width);
            bounds.2 = bounds.2.max(i % width + 1);
            bounds.3 = bounds.3.max(i / width + 1);
        }
    }
    let (left, top, right, bottom) = bounds;
    if left >= right || top >= bottom {
        return None;
    }
    let mut alpha = Vec::with_capacity((right - left) * (bottom - top));
    for y in top..bottom {
        alpha.extend((left..right).map(|x| rgba[(y * width + x) * channels + alpha_index]));
    }
    Some(Mask {
        width: right - left,
        height: bottom - top,
        alpha,
    })
}

thread_local! {
    static MASKS: RefCell<BTreeMap<&'static str, Option<Mask>>> = const { RefCell::new(BTreeMap::new()) };
}

struct Tile {
    size: usize,
    pixels: Vec<u8>,
}
type Point = (f64, f64);
impl Tile {
    fn new(size: usize) -> Self {
        Self {
            size,
            pixels: vec![0; size * size],
        }
    }

    fn paint(&mut self, coverage: impl Fn(Point) -> f64) {
        let scale = 16. / self.size as f64;
        for (i, pixel) in self.pixels.iter_mut().enumerate() {
            let at = (i % self.size, i / self.size);
            let samples = [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)];
            let alpha = samples
                .into_iter()
                .map(|(x, y)| coverage(((at.0 as f64 + x) * scale, (at.1 as f64 + y) * scale)))
                .sum::<f64>()
                / 4.;
            *pixel = (*pixel).max((alpha.clamp(0., 1.) * 255.).round() as u8);
        }
    }
    fn lines(&mut self, points: &[Point]) {
        self.paint(|p| {
            let covered = points.windows(2).any(|pair| {
                let (a, b) = (pair[0], pair[1]);
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0., 1.);
                (p.0 - a.0 - dx * t).hypot(p.1 - a.1 - dy * t) <= 0.72
            });
            f64::from(covered)
        });
    }
    fn rect(&mut self, l: f64, t: f64, r: f64, b: f64) {
        self.paint(|(x, y)| f64::from(x >= l && x < r && y >= t && y < b));
    }
    fn triangle(&mut self, a: Point, b: Point, c: Point) {
        fn side(p: Point, a: Point, b: Point) -> f64 {
            (p.0 - a.0) * (b.1 - a.1) - (p.1 - a.1) * (b.0 - a.0)
        }
        self.paint(|p| {
            let d = [side(p, a, b), side(p, b, c), side(p, c, a)];
            f64::from(d.iter().all(|&n| n >= 0.) || d.iter().all(|&n| n <= 0.))
        });
    }
    fn existing(&mut self, key: &'static str, compact: bool) {
        MASKS.with_borrow_mut(|masks| {
            if let Some(mask) = masks.entry(key).or_insert_with(|| decode(key)) {
                let side = if compact { 10.5 } else { 14. };
                let largest = mask.width.max(mask.height) as f64;
                let (w, h) = (
                    mask.width as f64 / largest * side,
                    mask.height as f64 / largest * side,
                );
                let (left, top) = (
                    (if compact { 6.8 } else { 8. }) - w / 2.,
                    (if compact { 6.8 } else { 8. }) - h / 2.,
                );
                self.paint(|(x, y)| {
                    let u = (x - left) / w;
                    let v = (y - top) / h;
                    if !(0. ..1.).contains(&u) || !(0. ..1.).contains(&v) {
                        return 0.;
                    }
                    f64::from(
                        mask.alpha[(v * mask.height as f64) as usize * mask.width
                            + (u * mask.width as f64) as usize],
                    ) / 255.
                });
            }
        });
    }
}

pub struct IconMasks {
    pub ink: Vec<u8>,
    pub accent: Vec<u8>,
}

pub fn paint(action: NoticeAction, size: usize) -> IconMasks {
    let (base, mark) = artwork(action);
    let mut tile = Tile::new(size);
    let mut accent = Tile::new(size);
    match base {
        Base::Existing(key) => tile.existing(key, !matches!(mark, Mark::None | Mark::Slash)),
        Base::Vector(Shape::Play) => tile.triangle((4., 2.), (13., 8.), (4., 14.)),
        Base::Vector(Shape::Pause) => {
            tile.rect(4., 2.5, 6.8, 13.5);
            tile.rect(9.2, 2.5, 12., 13.5);
        }
        Base::Vector(Shape::Next) => {
            tile.triangle((2., 3.), (11.5, 8.), (2., 13.));
            tile.rect(12., 3., 14., 13.);
        }
        Base::Vector(Shape::Previous) => {
            tile.triangle((14., 3.), (4.5, 8.), (14., 13.));
            tile.rect(2., 3., 4., 13.);
        }
        Base::Vector(Shape::Speaker) => {
            tile.lines(&[
                (1., 6.),
                (3.5, 6.),
                (7., 3.),
                (7., 13.),
                (3.5, 10.),
                (1., 10.),
                (1., 6.),
            ]);
        }
        Base::Vector(Shape::Battery) => {
            tile.lines(&[(1., 4.5), (12.5, 4.5), (12.5, 11.5), (1., 11.5), (1., 4.5)]);
            tile.lines(&[(14.6, 6.2), (14.6, 9.8)]);
        }
        Base::Vector(Shape::Download) => {
            tile.lines(&[(8., 1.5), (8., 10.5)]);
            tile.lines(&[(4.5, 7.), (8., 10.5), (11.5, 7.)]);
            tile.lines(&[(2.5, 11.), (2.5, 13.5), (13.5, 13.5), (13.5, 11.)]);
        }
    }
    match mark {
        Mark::None => {}
        Mark::Check => accent.lines(&[(9.5, 12.), (11.5, 14.), (15., 9.8)]),
        Mark::Slash => tile.lines(&[(2., 14.), (14., 2.)]),
        Mark::Plus => {
            tile.lines(&[(9.5, 8.), (15., 8.)]);
            tile.lines(&[(12.2, 5.2), (12.2, 10.8)]);
        }
        Mark::Minus => tile.lines(&[(9.5, 8.), (15., 8.)]),
        Mark::Cross => {
            tile.lines(&[(10., 6.), (14., 10.)]);
            tile.lines(&[(14., 6.), (10., 10.)]);
        }
        Mark::Waves => {
            tile.lines(&[(9., 6.), (10., 8.), (9., 10.)]);
            tile.lines(&[(12., 4.), (14., 8.), (12., 12.)]);
        }
        Mark::Up => tile.lines(&[(9.5, 13.5), (12.3, 10.5), (15., 13.5)]),
        Mark::Down => tile.lines(&[(9.5, 10.5), (12.3, 13.5), (15., 10.5)]),
        Mark::Low => tile.rect(3., 6.4, 4.7, 9.7),
        Mark::Warning => {
            accent.lines(&[(6.7, 6.2), (6.7, 8.1)]);
            accent.rect(6., 9., 7.4, 10.3);
        }
        Mark::Critical => {
            accent.lines(&[(5., 6.1), (8.7, 9.8)]);
            accent.lines(&[(8.7, 6.1), (5., 9.8)]);
        }
        Mark::Full => {
            for x in [3.0, 6.0, 9.0] {
                tile.rect(x, 6.3, x + 1.5, 9.7);
            }
        }
        Mark::Unavailable => tile.lines(&[(4.1, 7.9), (9.5, 7.9)]),
        Mark::Locate => accent.lines(&[
            (10., 10.),
            (14.5, 10.),
            (14.5, 14.5),
            (10., 14.5),
            (10., 10.),
        ]),
    }
    IconMasks {
        ink: tile.pixels,
        accent: accent.pixels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_preview_has_distinct_nonempty_artwork_at_supported_scales() {
        use std::collections::BTreeSet;
        for size in [14, 16, 20, 24, 32] {
            let mut seen = BTreeSet::new();
            for &(action, _) in nothingx_desktop::awareness::PREVIEWS {
                let icon = paint(action, size);
                assert!(
                    icon.ink.iter().chain(&icon.accent).any(|&alpha| alpha > 0),
                    "{action:?}"
                );
                assert!(
                    seen.insert((icon.ink, icon.accent)),
                    "{action:?} reused its entire icon"
                );
            }
        }
    }
}
