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

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Settings {
    pub theme: usize,
    pub drift: bool,
    /// Speak Pawline's plain answers aloud with a local engine. Off until switched on.
    pub speak: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { theme: 0, drift: true, speak: false }
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
    std::fs::write(path, format!("theme={}\ndrift={}\nspeak={}\n", THEMES[s.theme.min(THEMES.len() - 1)].name, if s.drift { "on" } else { "off" }, if s.speak { "on" } else { "off" }))
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
        save(&f, &Settings { theme: 2, drift: false, speak: true }).unwrap();
        assert_eq!(load(&f), Settings { theme: 2, drift: false, speak: true });
        std::fs::write(&f, "theme=Nope\ndrift=maybe\nspeak=maybe\njunk").unwrap();
        assert_eq!(load(&f), Settings { theme: 0, drift: true, speak: false });
        let t = std::fs::read_to_string(&f).unwrap();
        assert!(!t.contains("request"));
        assert_eq!(drift_offset(5.0, 300.0, false), 0.0);
        assert!(drift_offset(5.0, 300.0, true).abs() <= 300.0 * 0.45 + 0.01);
    }
}
