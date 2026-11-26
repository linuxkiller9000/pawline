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

pub const THEMES: [Theme; 5] = [
    Theme { name: "Dusk", bg: [(30, 28, 40), (38, 30, 58), (28, 34, 52)], user: (62, 54, 112), input: (52, 48, 70), accent: (92, 80, 170) },
    Theme { name: "Ocean", bg: [(18, 32, 48), (20, 52, 66), (24, 38, 74)], user: (28, 82, 112), input: (30, 56, 78), accent: (40, 120, 170) },
    Theme { name: "Forest", bg: [(20, 36, 30), (30, 50, 38), (28, 42, 48)], user: (40, 92, 66), input: (36, 62, 50), accent: (60, 130, 90) },
    Theme { name: "Rose", bg: [(44, 26, 36), (58, 30, 46), (40, 28, 52)], user: (120, 52, 82), input: (74, 42, 58), accent: (170, 80, 120) },
    // High contrast (item 13): near-black flat background (drift between near-identical stops is invisible),
    // every surface at least 7:1 against body, hint and note text (WCAG AAA, checked by the tests below).
    Theme { name: "Contrast", bg: [(6, 6, 9), (9, 9, 13), (6, 6, 9)], user: (0, 60, 120), input: (22, 22, 28), accent: (0, 80, 180) },
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
    /// Voice input blocked: the mic button records nothing. Off (allowed) by default; pressing the button is the user's act.
    pub mic_blocked: bool,
    /// Opt-in: keep the plain chat lines in a private file on this computer. Off by default.
    pub keep_history: bool,
    /// AI behaviour switch: when Pawline does not understand a request, the small embedded local model
    /// may suggest what was meant. On by default (the behaviour before this switch existed); it never
    /// uses the network. Off means the model is never woken and the plain parse error is the whole reply.
    pub assist: bool,
    /// Speech rate for spoken replies: index into SPEECH_RATES (item 20). Normal by default.
    /// A custom PAWLINE_TTS_CMD is not affected (its own flags are the user's).
    pub speech_rate: usize,
    /// Reduced motion (item 13): the slow colour drift is paused while this is on, whatever the
    /// drift switch says. Off by default (drift follows its own switch).
    pub reduced_motion: bool,
    /// File changes blocked (item 14): Pawline stays read-only - no plan that creates, changes,
    /// moves, deletes or saves a file is offered or run. Allowed by default.
    pub files_blocked: bool,
    /// Voice for spoken replies: index into VOICES (item 20). Default voice by default.
    pub voice: usize,
}

/// (label, words per minute) choices for spoken replies. 175 is espeak's own default.
pub const SPEECH_RATES: [(&str, u32); 3] = [("slow", 120), ("normal", 175), ("fast", 260)];

/// (label, espeak voice name) choices for spoken replies (item 20). The first entry passes no -v flag,
/// so the engine uses its own default voice. Applies to espeak/espeak-ng only: flite ignores it (its
/// voices are a different set) and a custom PAWLINE_TTS_CMD is untouched (its own flags are the user's).
pub const VOICES: [(&str, &str); 5] = [("default", ""), ("English (US)", "en-us"), ("English (British)", "en-gb"), ("higher (en+f3)", "en+f3"), ("lower (en+m2)", "en+m2")];

impl Default for Settings {
    fn default() -> Self {
        Settings { text_pct: 100, offline: false, theme: 0, drift: true, speak: false, mic_blocked: false, keep_history: false, assist: true, speech_rate: 1, reduced_motion: false, files_blocked: false, voice: 0 }
    }
}

/// "reset settings": text size, theme, drift and spoken replies go back to their defaults.
/// The privacy and permission switches (offline, mic block, saved conversation, file changes block)
/// and the reduced-motion choice are deliberately left as they are.
pub fn reset_look(s: &mut Settings) {
    let d = Settings::default();
    s.text_pct = d.text_pct;
    s.theme = d.theme;
    s.drift = d.drift;
    s.speak = d.speak;
    s.speech_rate = d.speech_rate;
    s.voice = d.voice;
    // Reduced motion is an accessibility choice, not a look preference: left as the user set it.
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
                Some(("mic", v)) => s.mic_blocked = v.trim() == "blocked",
                Some(("history", v)) => s.keep_history = v.trim() == "on",
                Some(("assist", v)) => s.assist = v.trim() != "off",
                Some(("speech_rate", v)) => s.speech_rate = SPEECH_RATES.iter().position(|(n, _)| n == &v.trim()).unwrap_or(1),
                Some(("motion", v)) => s.reduced_motion = v.trim() == "reduced",
                Some(("voice", v)) => s.voice = VOICES.iter().position(|(_, name)| name == &v.trim()).unwrap_or(0),
                Some(("files", v)) => s.files_blocked = v.trim() == "blocked",
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
    std::fs::write(path, format!("theme={}\ndrift={}\nspeak={}\ntext={}\noffline={}\nmic={}\nhistory={}\nassist={}\nspeech_rate={}\nmotion={}\nfiles={}\nvoice={}\n", THEMES[s.theme.min(THEMES.len() - 1)].name, if s.drift { "on" } else { "off" }, if s.speak { "on" } else { "off" }, s.text_pct, if s.offline { "on" } else { "off" }, if s.mic_blocked { "blocked" } else { "allowed" }, if s.keep_history { "on" } else { "off" }, if s.assist { "on" } else { "off" }, SPEECH_RATES[s.speech_rate.min(SPEECH_RATES.len() - 1)].0, if s.reduced_motion { "reduced" } else { "full" }, if s.files_blocked { "blocked" } else { "allowed" }, VOICES[s.voice.min(VOICES.len() - 1)].1))
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
    fn speech_rate_survives_roundtrip_and_bad_values_fall_back() {
        let d = std::env::temp_dir().join(format!("pawline-sr-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("ui-settings.txt");
        // Old file without the key: normal.
        std::fs::write(&p, "theme=Ocean\ndrift=on\n").unwrap();
        assert_eq!(load(&p).speech_rate, 1);
        // Round trip through the real save/load.
        let mut s = Settings::default();
        s.speech_rate = 0;
        save(&p, &s).unwrap();
        assert_eq!(load(&p).speech_rate, 0);
        s.speech_rate = 2;
        save(&p, &s).unwrap();
        let l = load(&p);
        assert_eq!(l.speech_rate, 2);
        assert!(std::fs::read_to_string(&p).unwrap().contains("speech_rate=fast"));
        // Unknown value falls back to normal without losing the other keys.
        std::fs::write(&p, "speech_rate=ludicrous\nspeak=on\n").unwrap();
        let l = load(&p);
        assert_eq!(l.speech_rate, 1);
        assert!(l.speak);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn reset_look_restores_look_and_keeps_every_privacy_switch() {
        let mut s = Settings { text_pct: 150, offline: true, theme: 2, drift: false, speak: true, mic_blocked: true, keep_history: true, assist: false, speech_rate: 2, reduced_motion: true, files_blocked: true, voice: 3 };
        reset_look(&mut s);
        assert_eq!(s, Settings { offline: true, mic_blocked: true, keep_history: true, assist: false, speech_rate: 1, reduced_motion: true, files_blocked: true, ..Settings::default() }, "privacy and permission switches and the reduced-motion choice survive a look reset (the voice resets)");
    }

    #[test]
    fn settings_round_trip_and_bad_files_fall_back() {
        let d = std::env::temp_dir().join(format!("pawline-theme-{}", std::process::id()));
        let f = d.join("ui-settings.txt");
        assert_eq!(load(&f), Settings::default());
        save(&f, &Settings { text_pct: 100, offline: false, theme: 2, drift: false, speak: true, mic_blocked: false, keep_history: false, assist: false, speech_rate: 1, reduced_motion: false, files_blocked: false, voice: 0 }).unwrap();
        assert_eq!(load(&f), Settings { text_pct: 100, offline: false, theme: 2, drift: false, speak: true, mic_blocked: false, keep_history: false, assist: false, speech_rate: 1, reduced_motion: false, files_blocked: false, voice: 0 });
        std::fs::write(&f, "theme=Nope\ndrift=maybe\nspeak=maybe\nassist=maybe\njunk").unwrap();
        assert_eq!(load(&f), Settings { text_pct: 100, offline: false, theme: 0, drift: true, speak: false, mic_blocked: false, keep_history: false, assist: true, speech_rate: 1, reduced_motion: false, files_blocked: false, voice: 0 });
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

    /// The high-contrast theme must clear WCAG AAA (7:1) for body, hint and note text on every
    /// surface it owns, and for the white text on its accent and user-bubble fills.
    #[test]
    fn high_contrast_theme_meets_7_to_1() {
        let t = THEMES.iter().find(|t| t.name == "Contrast").expect("Contrast theme");
        let mut surfaces: Vec<(&str, Rgb)> = vec![("card", CARD_BG)];
        for (i, c) in t.bg.iter().enumerate() {
            surfaces.push((match i { 0 => "bg0", 1 => "bg1", _ => "bg2" }, *c));
        }
        surfaces.push(("input", t.input));
        for (name, bg) in &surfaces {
            for (what, fg) in [("hint", HINT), ("note", NOTE), ("body", (235, 230, 255))] {
                let r = contrast(fg, *bg);
                assert!(r >= 7.0, "Contrast theme: {what} on {name}: {r:.2} < 7");
            }
        }
        assert!(contrast((255, 255, 255), t.user) >= 7.0, "white on Contrast user bubble");
        assert!(contrast((255, 255, 255), t.accent) >= 7.0, "white on Contrast accent");
    }

    #[test]
    fn contrast_function_matches_known_values() {
        assert!((contrast((0, 0, 0), (255, 255, 255)) - 21.0).abs() < 0.01);
        assert!((contrast((119, 119, 119), (255, 255, 255)) - 4.48).abs() < 0.02);
    }
}

#[cfg(test)]
mod reduced_motion_tests {
    use super::*;

    #[test]
    fn reduced_motion_round_trips_and_bad_values_stay_full() {
        let d = std::env::temp_dir().join(format!("pawline-rm-{}", std::process::id()));
        let f = d.join("ui-settings.txt");
        let on = Settings { reduced_motion: true, ..Settings::default() };
        save(&f, &on).unwrap();
        assert!(load(&f).reduced_motion, "motion=reduced survives a save/load round trip");
        let off = Settings::default();
        save(&f, &off).unwrap();
        assert!(!load(&f).reduced_motion, "motion=full is the default state");
        std::fs::write(&f, "motion=whatever\n").unwrap();
        assert!(!load(&f).reduced_motion, "only the word reduced pauses drift; anything else keeps full motion");
        std::fs::write(&f, "theme=Dusk\n").unwrap();
        assert!(!load(&f).reduced_motion, "an old settings file with no motion line keeps full motion");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn reset_look_keeps_reduced_motion() {
        let mut s = Settings { reduced_motion: true, theme: 2, drift: false, ..Settings::default() };
        reset_look(&mut s);
        assert!(s.reduced_motion, "reset settings does not touch the reduced-motion choice");
        assert_eq!(s.theme, 0);
        assert!(s.drift);
    }
}

#[cfg(test)]
mod voice_tests {
    use super::*;

    #[test]
    fn voice_round_trips_and_bad_values_fall_back_to_default() {
        let d = std::env::temp_dir().join(format!("pawline-voice-{}", std::process::id()));
        let f = d.join("ui-settings.txt");
        save(&f, &Settings { voice: 1, ..Settings::default() }).unwrap();
        assert_eq!(load(&f).voice, 1, "voice=en-us survives a save/load round trip");
        std::fs::write(&f, "voice=banana\n").unwrap();
        assert_eq!(load(&f).voice, 0, "an unknown voice falls back to the engine default");
        std::fs::write(&f, "theme=Dusk\n").unwrap();
        assert_eq!(load(&f).voice, 0, "an old settings file with no voice line keeps the default voice");
        std::fs::remove_dir_all(&d).ok();
    }
}

#[cfg(test)]
mod text_size_tests {
    use super::*;

    #[test]
    fn text_size_round_trips_and_bad_values_fall_back_to_100() {
        let d = std::env::temp_dir().join(format!("pawline-textsize-{}", std::process::id()));
        let f = d.join("ui-settings.txt");
        let blocked = Settings { mic_blocked: true, ..Settings::default() };
        save(&f, &blocked).unwrap();
        assert!(load(&f).mic_blocked);
        std::fs::write(&f, "mic=whatever\n").unwrap();
        assert!(!load(&f).mic_blocked, "only the word blocked blocks; anything else is the default");
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

/// Item 20 "settings survive updates": the file outlives the binary, so every version
/// must read what older and newer versions wrote without losing the user's choices.
#[cfg(test)]
mod update_survival_tests {
    use super::*;

    fn tmp(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let d = std::env::temp_dir().join(format!("pawline-upd-{tag}-{}", std::process::id()));
        (d.clone(), d.join("ui-settings.txt"))
    }

    #[test]
    fn old_version_partial_file_keeps_its_values_and_defaults_the_rest() {
        let (d, f) = tmp("old");
        std::fs::create_dir_all(&d).unwrap();
        // A file from the first version, which knew only theme and drift.
        std::fs::write(&f, "theme=Ocean\ndrift=off\n").unwrap();
        let s = load(&f);
        assert_eq!(s.theme, 1, "Ocean kept");
        assert!(!s.drift, "drift kept");
        assert_eq!(s.text_pct, 100);
        assert!(!s.offline && !s.speak && !s.mic_blocked && !s.keep_history, "later keys default");
        assert!(s.assist, "a file from before the AI-suggestions switch keeps suggestions on (the behaviour it had)");
        // The upgrade path: loading the old file and saving it again preserves the old choices.
        save(&f, &s).unwrap();
        let s2 = load(&f);
        assert_eq!(s2.theme, 1);
        assert!(!s2.drift);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn future_version_unknown_keys_are_ignored_and_known_keys_still_load() {
        let (d, f) = tmp("future");
        std::fs::create_dir_all(&d).unwrap();
        // A file from a newer version with settings this build does not know.
        std::fs::write(&f, "language=ar\ntheme=Forest\nvoice=fast\ndrift=off\nfuture_toggle=on\n").unwrap();
        let s = load(&f);
        assert_eq!(s.theme, 2, "Forest kept");
        assert!(!s.drift);
        // Note: a re-save writes only the keys this build knows, dropping unknown future keys.
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn one_corrupt_value_falls_back_without_losing_the_other_keys() {
        let (d, f) = tmp("corrupt");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(&f, "theme=Rose\ntext=banana\nmic=garbage\ndrift=off\nhistory=on\n").unwrap();
        let s = load(&f);
        assert_eq!(s.theme, 3, "Rose kept");
        assert!(!s.drift, "drift kept");
        assert!(s.keep_history, "history kept");
        assert_eq!(s.text_pct, 100, "bad text size falls back");
        assert!(!s.mic_blocked, "bad mic value falls back");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn duplicate_keys_last_line_wins() {
        let (d, f) = tmp("dup");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(&f, "theme=Ocean\ntheme=Rose\ndrift=off\ndrift=on\n").unwrap();
        let s = load(&f);
        assert_eq!(s.theme, 3, "the later theme line wins");
        assert!(s.drift, "the later drift line wins");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn windows_line_endings_and_binary_garbage_do_not_break_loading() {
        let (d, f) = tmp("crlf");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(&f, "theme=Forest\r\ndrift=off\r\n").unwrap();
        let s = load(&f);
        assert_eq!(s.theme, 2);
        assert!(!s.drift);
        std::fs::write(&f, [0xff, 0xfe, 0x00, 0x9c, b'=']).unwrap();
        assert_eq!(load(&f), Settings::default(), "non-UTF-8 falls back to defaults, no panic");
        let _ = std::fs::remove_dir_all(&d);
    }
}
