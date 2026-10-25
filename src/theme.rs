//! Colors, and the background the user picks (saved in data/settings.tsv).

use ratatui::style::Color as Tc;
use std::sync::atomic::{AtomicU32, Ordering};

pub const ACCENT: Tc = Tc::Cyan;
/// Background presets, cycled from the menu. `NO_BG` leaves the terminal's own background.
pub const NO_BG: u32 = u32::MAX;
pub const BACKGROUNDS: [(&str, u32); 3] =
    [("dark", 0x242836), ("black", 0x000000), ("none", NO_BG)];

/// Current background as 0xRRGGBB or `NO_BG`; global because every panel paints it.
pub static BG: AtomicU32 = AtomicU32::new(0x242836);

pub fn bg() -> Tc {
    match BG.load(Ordering::Relaxed) {
        NO_BG => Tc::Reset,
        c => Tc::Rgb((c >> 16) as u8, (c >> 8) as u8, c as u8),
    }
}

/// A preset name or a custom "#rrggbb".
pub fn parse_bg(s: &str) -> Option<u32> {
    match BACKGROUNDS.iter().find(|(name, _)| *name == s) {
        Some(&(_, c)) => Some(c),
        None => u32::from_str_radix(s.strip_prefix('#')?, 16)
            .ok()
            .filter(|_| s.len() == 7),
    }
}

pub fn bg_name(c: u32) -> String {
    BACKGROUNDS
        .iter()
        .find(|&&(_, v)| v == c)
        .map_or_else(|| format!("#{c:06x}"), |(name, _)| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_settings_parse() {
        assert_eq!(parse_bg("dark"), Some(0x242836));
        assert_eq!(parse_bg("none"), Some(NO_BG));
        assert_eq!(parse_bg("#1e1e2e"), Some(0x1e1e2e));
        assert_eq!(parse_bg("#12345"), None);
        assert_eq!(parse_bg("purple"), None);
        assert_eq!(bg_name(0x1e1e2e), "#1e1e2e");
        assert_eq!(bg_name(NO_BG), "none");
    }
}
