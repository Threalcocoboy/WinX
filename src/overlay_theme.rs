//! Native palette and metrics are read from the same CSS custom properties
//! the WinX panel uses. Only literal values in its two :root blocks are read.
use std::{sync::OnceLock, time::Duration};

const STYLES: &str = include_str!("../ui/src/popover/styles.css");

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub f64);

impl Rgba {
    pub fn hex(value: &str) -> Option<Self> {
        let value = value.strip_prefix('#')?;
        if value.len() != 6 || !value.bytes().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let color = u32::from_str_radix(value, 16).ok()?;
        Some(Self(
            (color >> 16) as u8,
            (color >> 8) as u8,
            color as u8,
            1.,
        ))
    }

    fn css(value: &str) -> Self {
        if let Some(color) = Self::hex(value) {
            return color;
        }
        let body = value
            .strip_prefix("rgba(")
            .and_then(|v| v.strip_suffix(')'))
            .expect("CSS rgba token");
        let parts: Vec<_> = body.split(',').map(str::trim).collect();
        let byte = |index: usize| parts[index].parse::<u8>().expect("CSS rgba channel");
        Self(
            byte(0),
            byte(1),
            byte(2),
            parts[3].parse().expect("CSS opacity"),
        )
    }

    pub fn native(self) -> (u8, u8, u8, u8) {
        (self.0, self.1, self.2, (255. * self.3).round() as u8)
    }
}

pub struct Design {
    pub fill: Rgba,
    pub material: Rgba,
    pub text: Rgba,
    pub secondary: Rgba,
    pub border: Rgba,
    pub chip: Rgba,
    pub warning: Rgba,
    pub accent: Rgba,
    pub radius: f64,
    pub padding: f64,
    pub inset: f64,
    pub gap: f64,
    pub badge: f64,
    pub badge_radius: f64,
    pub icon: f64,
    pub font: &'static str,
    pub title: f64,
    pub subtitle: f64,
    pub weight: i32,
    pub stroke: f64,
    pub animation: Duration,
    pub slide: f64,
    pub shadow: f64,
}

impl Design {
    fn read(dark: bool) -> Self {
        let mut blocks = STYLES
            .split(":root {")
            .skip(1)
            .filter_map(|block| block.split_once('}').map(|(body, _)| body));
        let light = blocks.next().expect("WinX light theme");
        let dark_block = blocks.next().expect("WinX dark theme");
        let token = |name: &str| {
            fn find<'a>(block: &'a str, name: &str) -> Option<&'a str> {
                block
                    .lines()
                    .filter_map(|line| line.trim().split_once(':'))
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| value.trim().trim_end_matches(';'))
            }
            dark.then(|| find(dark_block, name))
                .flatten()
                .or_else(|| find(light, name))
                .expect("shared CSS token")
        };
        let px = |name| {
            token(name)
                .strip_suffix("px")
                .expect("CSS px token")
                .parse()
                .expect("CSS px value")
        };
        let color = |name| Rgba::css(token(name));
        Self {
            fill: color("--winx-panel-fill"),
            material: color("--winx-window-material"),
            text: color("--text"),
            secondary: color("--text-secondary"),
            border: color("--separator"),
            chip: color("--chip"),
            warning: color("--batt-low"),
            accent: color("--accent"),
            radius: px("--winx-panel-radius"),
            padding: px("--winx-panel-padding"),
            inset: px("--winx-header-inset"),
            gap: px("--winx-header-gap"),
            badge: px("--winx-badge-size"),
            badge_radius: px("--winx-badge-radius"),
            icon: px("--winx-icon-size"),
            font: token("--winx-font-windows").trim_matches('"'),
            title: px("--winx-title-size"),
            subtitle: px("--winx-subtitle-size"),
            weight: token("--winx-title-weight").parse().expect("CSS weight"),
            stroke: px("--winx-stroke"),
            slide: px("--winx-view-slide") / 2.,
            animation: Duration::from_millis(
                token("--winx-animation")
                    .trim_end_matches("ms")
                    .parse()
                    .expect("CSS ms value"),
            ),
            shadow: token("--winx-shadow-alpha")
                .parse()
                .expect("CSS shadow opacity"),
        }
    }
}

pub fn design(dark: bool) -> &'static Design {
    static LIGHT: OnceLock<Design> = OnceLock::new();
    static DARK: OnceLock<Design> = OnceLock::new();
    if dark {
        DARK.get_or_init(|| Design::read(true))
    } else {
        LIGHT.get_or_init(|| Design::read(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlay_and_panel_share_the_css_palette_typography_and_timing() {
        let light = design(false);
        let dark = design(true);
        assert_ne!(light.fill, dark.fill);
        assert_ne!(light.text, dark.text);
        assert_eq!(light.material.native(), (240, 240, 240, 190));
        assert_eq!(dark.material.native(), (36, 36, 36, 190));
        assert_eq!(
            (dark.radius, dark.title, dark.subtitle, dark.weight),
            (12., 13., 11., 600)
        );
        assert_eq!(light.animation, Duration::from_millis(180));
        assert_eq!(light.font, dark.font);
    }
}
