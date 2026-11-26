//! Colour themes and the two UI settings (theme, slow colour drift). Pure state, no window.
//! Stored as a tiny text file ("theme=Ocean", "drift=on") next to Pawline's other local data; it holds no request text.

use std::path::Path;

pub type Rgb = (u8, u8, u8);

pub struct Theme {
    pub name: &'static str,
    /// Three colours the panel background fades between.
    pub bg: [Rgb; 3],
    pub user: Rgb,
    pub input: Rgb,
    pub accent: Rgb,
}

pub const THEMES: [Theme; 4] = [
    Theme { name: "Dusk", bg: [(30, 28, 40), (38, 30, 58), (28, 34, 52)], user: (62, 54, 112), input: (52, 48, 70), accent: (92, 80, 170) },
    Theme { name: "Ocean", bg: [(18, 32, 48), (20, 52, 66), (24, 38, 74)], user: (28, 82, 112), input: (30, 56, 78), accent: (40, 120, 170) },
    Theme { name: "Forest", bg: [(20, 36, 30), (30, 50, 38), (28, 42, 48)], user: (40, 92, 66), input: (36, 62, 50), accent: (60, 130, 90) },
    Theme { name: "Rose", bg: [(44, 26, 36), (58, 30, 46), (40, 28, 52)], user: (120, 52, 82), input: (74, 42, 58), accent: (170, 80, 120) },
];

/// Quiet text (hints, placeholder) and the label of a disabled button. Chosen so each is at least 4.5:1 on every panel colour
/// and input surface below (checked by `quiet_text_meets_4_5_to_1_on_every_surface`).
pub const HINT: Rgb = (172, 167, 197);
pub const DISABLED_FG: Rgb = (175, 175, 185);
pub const DISABLED_BG: Rgb = (60, 60, 66);
pub const NOTE: Rgb = (190, 185, 215);
pub const CARD_BG: Rgb = (29, 20, 64);

/// WCAG 2.x contrast ratio between two sRGB colours.
pub fn contrast(a: Rgb, b: Rgb) -> f32 {
    fn lum(c: Rgb) -> f32 {
        let f = |v: u8| {
            let v = v as f32 / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * f(c.0) + 0.7152 * f(c.1) + 0.0722 * f(c.2)
    }
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Text size choices in percent (item 13). 100 is the original size.
pub const TEXT_SIZES: [u32; 3] = [100, 125, 150];
static TEXT_PCT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(100);
pub fn set_text_pct(p: u32) {
    TEXT_PCT.store(if TEXT_SIZES.contains(&p) { p } else { 100 }, std::sync::atomic::Ordering::Relaxed);
}
/// Current text scale factor (1.0, 1.25 or 1.5). One window per process, so a process-wide value is enough.
pub fn text_scale() -> f32 {
    TEXT_PCT.load(std::sync::atomic::Ordering::Relaxed) as f32 / 100.0
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Settings {
    pub text_pct: u32,
    /// Offline mode: no network request is made at all (see pawline-core netgate). Off by default.
    pub offline: bool,
    pub theme: usize,
    pub drift: bool,
    /// Speak Pawline's plain answers aloud with a local engine. Off until switched on.
    pub speak: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { text_pct: 100, offline: false, theme: 0, drift: true, speak: false }
    }
}

pub fn load(path: &Path) -> Settings {
    let mut s = Settings::default();
    if let Ok(t) = std::fs::read_to_string(path) {
        for l in t.lines() {
            match l.split_once('=') {
                Some(("theme", v)) => {
                    if let Some(i) = THEMES.iter().position(|t| t.name == v.trim()) {
                        s.theme = i;
                    }
                }
                Some(("drift", v)) => s.drift = v.trim() != "off",
                Some(("speak", v)) => s.speak = v.trim() == "on",
                Some(("offline", v)) => s.offline = v.trim() == "on",
                Some(("text", v)) => s.text_pct = v.trim().parse().ok().filter(|p| TEXT_SIZES.contains(p)).unwrap_or(100),
                _ => {}
            }
        }
    }
    s
}

pub fn save(path: &Path, s: &Settings) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(path, format!("theme={}\ndrift={}\nspeak={}\ntext={}\noffline={}\n", THEMES[s.theme.min(THEMES.len() - 1)].name, if s.drift { "on" } else { "off" }, if s.speak { "on" } else { "off" }, s.text_pct, if s.offline { "on" } else { "off" }))
}

/// Gradient offset along the diagonal for a moment in time: a slow back-and-forth, 0 when drift is off.
pub fn drift_offset(secs: f32, w: f32, drift: bool) -> f32 {
    if !drift {
        return 0.0;
    }
    (secs * 0.35).sin() * w * 0.45
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_round_trip_and_bad_files_fall_back() {
        let d = std::env::temp_dir().join(format!("pawline-theme-{}", std::process::id()));
        let f = d.join("ui-settings.txt");
        assert_eq!(load(&f), Settings::default());
        save(&f, &Settings { text_pct: 100, offline: false, theme: 2, drift: false, speak: true }).unwrap();
        assert_eq!(load(&f), Settings { text_pct: 100, offline: false, theme: 2, drift: false, speak: true });
        std::fs::write(&f, "theme=Nope\ndrift=maybe\nspeak=maybe\njunk").unwrap();
        assert_eq!(load(&f), Settings { text_pct: 100, offline: false, theme: 0, drift: true, speak: false });
        let t = std::fs::read_to_string(&f).unwrap();
        assert!(!t.contains("request"));
        assert_eq!(drift_offset(5.0, 300.0, false), 0.0);
        assert!(drift_offset(5.0, 300.0, true).abs() <= 300.0 * 0.45 + 0.01);
    }
}

#[cfg(test)]
mod contrast_tests {
    use super::*;

    #[test]
    fn quiet_text_meets_4_5_to_1_on_every_surface() {
        let mut surfaces: Vec<(String, Rgb)> = vec![("card".into(), CARD_BG)];
        for t in THEMES.iter() {
            for (i, c) in t.bg.iter().enumerate() {
                surfaces.push((format!("{} bg{i}", t.name), *c));
            }
            surfaces.push((format!("{} input", t.name), t.input));
        }
        for (name, bg) in &surfaces {
            for (what, fg) in [("hint", HINT), ("note", NOTE), ("body", (235, 230, 255))] {
                let r = contrast(fg, *bg);
                assert!(r >= 4.5, "{what} on {name}: {r:.2}");
            }
        }
        for t in THEMES.iter() {
            let r = contrast((235, 230, 255), t.user);
            assert!(r >= 4.5, "text on {} user bubble: {r:.2}", t.name);
        }
        assert!(contrast(DISABLED_FG, DISABLED_BG) >= 4.5);
    }

    #[test]
    fn contrast_function_matches_known_values() {
        assert!((contrast((0, 0, 0), (255, 255, 255)) - 21.0).abs() < 0.01);
        assert!((contrast((119, 119, 119), (255, 255, 255)) - 4.48).abs() < 0.02);
    }
}

#[cfg(test)]
mod text_size_tests {
    use super::*;

    #[test]
    fn text_size_round_trips_and_bad_values_fall_back_to_100() {
        let d = std::env::temp_dir().join(format!("pawline-textsize-{}", std::process::id()));
        let f = d.join("ui-settings.txt");
        let off = Settings { offline: true, ..Settings::default() };
        save(&f, &off).unwrap();
        assert!(load(&f).offline);
        std::fs::write(&f, "offline=maybe\n").unwrap();
        assert!(!load(&f).offline, "anything but on is off");
        for p in TEXT_SIZES {
            let s = Settings { text_pct: p, ..Settings::default() };
            save(&f, &s).unwrap();
            assert_eq!(load(&f).text_pct, p);
        }
        for bad in ["text=300", "text=abc", "text=", "text=-5"] {
            std::fs::write(&f, format!("theme=Dusk\n{bad}\n")).unwrap();
            assert_eq!(load(&f).text_pct, 100, "{bad}");
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
