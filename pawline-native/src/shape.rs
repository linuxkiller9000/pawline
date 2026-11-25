//! Text shaping and bidirectional layout for the CPU-rendered UI. Pure state, no window, so it is unit-tested.
//!
//! Pipeline: replace bidi control characters with a visible marker (index-preserving, so nothing can reorder text
//! behind the user's back), split into segments, resolve bidi levels (unicode-bidi), pick a font per run (Latin
//! UI font or the embedded Arabic font), shape with rustybuzz (joining forms, ligatures, marks), and record for
//! every input char its horizontal extent so carets, selections and clicks work on mixed-direction text.

use std::collections::HashMap;
use unicode_bidi::{BidiInfo, Level};

static LATIN: &[u8] = include_bytes!("../assets/font.ttf");
static ARABIC: &[u8] = include_bytes!("../assets/arabic.ttf");

/// How a line is laid out.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Mode {
    /// Paragraph direction from the first strong character (chat text, prompt).
    Auto,
    /// Always left-to-right, split at "/" so every path part stays in the order written. Used for plans, confirm
    /// cards and path lines: an Arabic file name can never reorder the folders around it.
    Strict,
}

#[derive(Clone, Debug)]
pub struct G {
    pub font: u8,
    pub gid: u16,
    pub x: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Laid {
    pub glyphs: Vec<G>,
    pub width: f32,
    /// Paragraph direction is right-to-left: callers right-align the line.
    pub rtl: bool,
    /// Per input char: (left, right) extent and whether the char sits in a right-to-left run.
    pub ext: Vec<(f32, f32, bool)>,
}

pub struct Shaper {
    fd: [fontdue::Font; 2],
    face: [rustybuzz::Face<'static>; 2],
    upem: [f32; 2],
    cache: HashMap<(String, u32, Mode), Laid>,
    glyphs: HashMap<(u8, u16, u32), (fontdue::Metrics, Vec<u8>)>,
}

pub fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{061C}')
}

/// True when the text contains characters that can change the visual order of what follows them.
pub fn has_bidi_control(s: &str) -> bool {
    s.chars().any(is_bidi_control)
}

fn is_arabic_script(c: char) -> bool {
    matches!(c as u32, 0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF)
}

/// Strong right-to-left text in the line (decides right alignment in Auto mode).
pub fn has_rtl(s: &str) -> bool {
    s.chars().any(|c| is_arabic_script(c) || matches!(c as u32, 0x0590..=0x05FF))
}

impl Shaper {
    pub fn new() -> Shaper {
        let fd = [
            fontdue::Font::from_bytes(LATIN, fontdue::FontSettings::default()).unwrap(),
            fontdue::Font::from_bytes(ARABIC, fontdue::FontSettings::default()).unwrap(),
        ];
        let face = [rustybuzz::Face::from_slice(LATIN, 0).unwrap(), rustybuzz::Face::from_slice(ARABIC, 0).unwrap()];
        let upem = [face[0].units_per_em() as f32, face[1].units_per_em() as f32];
        Shaper { fd, face, upem, cache: HashMap::new(), glyphs: HashMap::new() }
    }

    fn font_for(&self, c: char, prev: u8) -> u8 {
        let has = |f: usize| self.fd[f].lookup_glyph_index(c) != 0;
        if is_arabic_script(c) {
            1
        } else if has(prev as usize) && !c.is_alphanumeric() {
            prev // spaces and punctuation stay with the neighbouring text
        } else if has(0) {
            0
        } else if has(1) {
            1
        } else {
            0
        }
    }

    pub fn layout(&mut self, text: &str, px: f32, mode: Mode) -> Laid {
        let key = (text.to_string(), px.to_bits(), mode);
        if let Some(l) = self.cache.get(&key) {
            return l.clone();
        }
        let l = self.layout_uncached(text, px, mode);
        if self.cache.len() > 4000 {
            self.cache.clear();
        }
        self.cache.insert(key, l.clone());
        l
    }

    fn layout_uncached(&self, text: &str, px: f32, mode: Mode) -> Laid {
        // one char in, one char out: controls become a visible marker, line breaks become spaces
        let chars: Vec<char> = text.chars().map(|c| if is_bidi_control(c) { '\u{FFFD}' } else if c.is_control() { ' ' } else { c }).collect();
        let mut out = Laid { ext: vec![(0.0, 0.0, false); chars.len()], ..Default::default() };
        // segments as (first char, one past last char)
        let mut segs: Vec<(usize, usize)> = Vec::new();
        match mode {
            Mode::Auto => segs.push((0, chars.len())),
            Mode::Strict => {
                let mut s = 0;
                for (i, c) in chars.iter().enumerate() {
                    if *c == '/' {
                        if i > s {
                            segs.push((s, i));
                        }
                        segs.push((i, i + 1));
                        s = i + 1;
                    }
                }
                if s < chars.len() {
                    segs.push((s, chars.len()));
                }
            }
        }
        let mut pen = 0.0f32;
        for (cs, ce) in segs {
            if ce <= cs {
                continue;
            }
            let seg: String = chars[cs..ce].iter().collect();
            let level = match mode {
                Mode::Auto => None,
                Mode::Strict => Some(Level::ltr()),
            };
            let info = BidiInfo::new(&seg, level);
            let Some(para) = info.paragraphs.first() else { continue };
            if cs == 0 && mode == Mode::Auto {
                out.rtl = para.level.is_rtl();
            }
            let (levels, runs) = info.visual_runs(para, para.range.clone());
            // byte offset in `seg` -> char index within the whole text
            let mut b2c = vec![0usize; seg.len() + 1];
            for (ci, (bi, c)) in seg.char_indices().enumerate() {
                for k in 0..c.len_utf8() {
                    b2c[bi + k] = cs + ci;
                }
            }
            b2c[seg.len()] = ce;
            for run in runs {
                let rtl = levels[run.start].is_rtl();
                // split the run by font
                let mut parts: Vec<(usize, usize, u8)> = Vec::new();
                let mut prev = 0u8;
                for (bi, c) in seg[run.clone()].char_indices() {
                    let f = self.font_for(c, prev);
                    prev = f;
                    let b = run.start + bi;
                    match parts.last_mut() {
                        Some(p) if p.2 == f => p.1 = b + c.len_utf8(),
                        _ => parts.push((b, b + c.len_utf8(), f)),
                    }
                }
                if rtl {
                    // parts were collected logically; an RTL run is laid out right to left
                    parts.reverse();
                }
                for (b0, b1, f) in parts {
                    pen = self.shape_part(&seg, b0, b1, f, rtl, px, pen, &b2c, &mut out);
                }
            }
        }
        out.width = pen;
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn shape_part(&self, seg: &str, b0: usize, b1: usize, f: u8, rtl: bool, px: f32, mut pen: f32, b2c: &[usize], out: &mut Laid) -> f32 {
        let sub = &seg[b0..b1];
        let mut buf = rustybuzz::UnicodeBuffer::new();
        buf.push_str(sub);
        buf.set_direction(if rtl { rustybuzz::Direction::RightToLeft } else { rustybuzz::Direction::LeftToRight });
        buf.guess_segment_properties();
        let gb = rustybuzz::shape(&self.face[f as usize], &[], buf);
        let scale = px / self.upem[f as usize];
        // cluster start (byte in sub) -> x extent
        let mut clusters: Vec<(usize, f32, f32)> = Vec::new();
        for (i, p) in gb.glyph_infos().iter().zip(gb.glyph_positions()) {
            let adv = p.x_advance as f32 * scale;
            let x = pen + p.x_offset as f32 * scale;
            out.glyphs.push(G { font: f, gid: i.glyph_id as u16, x });
            let cl = i.cluster as usize;
            let (lo, hi) = (pen.min(pen + adv), pen.max(pen + adv));
            match clusters.iter_mut().find(|c| c.0 == cl) {
                Some(c) => {
                    c.1 = c.1.min(lo);
                    c.2 = c.2.max(hi);
                }
                None => clusters.push((cl, lo, hi)),
            }
            pen += adv;
        }
        clusters.sort_by_key(|c| c.0);
        for k in 0..clusters.len() {
            let start = clusters[k].0;
            let end = clusters.get(k + 1).map_or(sub.len(), |c| c.0);
            let idx: Vec<usize> = sub[start..end].char_indices().map(|(bi, _)| b2c[b0 + start + bi]).collect();
            let (lo, hi) = (clusters[k].1, clusters[k].2);
            let m = idx.len().max(1) as f32;
            let wdt = (hi - lo) / m;
            for (j, ci) in idx.iter().enumerate() {
                let slot = if rtl { idx.len() - 1 - j } else { j } as f32;
                out.ext[*ci] = (lo + slot * wdt, lo + (slot + 1.0) * wdt, rtl);
            }
        }
        pen
    }

    pub fn measure(&mut self, text: &str, px: f32, mode: Mode) -> f32 {
        self.layout(text, px, mode).width
    }

    pub fn glyph(&mut self, font: u8, gid: u16, px: f32) -> &(fontdue::Metrics, Vec<u8>) {
        let key = (font, gid, px.to_bits());
        if !self.glyphs.contains_key(&key) {
            let g = self.fd[font as usize].rasterize_indexed(gid, px);
            self.glyphs.insert(key, g);
        }
        &self.glyphs[&key]
    }
}

impl Laid {
    /// X of the caret before char `i` (i == number of chars: after the last one).
    pub fn caret_x(&self, i: usize) -> f32 {
        if self.ext.is_empty() {
            return if self.rtl { self.width } else { 0.0 };
        }
        if i < self.ext.len() {
            let (l, r, rtl) = self.ext[i];
            if rtl { r } else { l }
        } else {
            let (l, r, rtl) = self.ext[self.ext.len() - 1];
            if rtl { l } else { r }
        }
    }

    /// Caret index nearest to an x position (relative to the line's left edge).
    pub fn index_at(&self, x: f32) -> usize {
        let n = self.ext.len();
        if n == 0 {
            return 0;
        }
        for (i, &(l, r, rtl)) in self.ext.iter().enumerate() {
            if x >= l && x < r {
                let first_half = (x - l) < (r - l) / 2.0;
                let leading = if rtl { !first_half } else { first_half };
                return if leading { i } else { i + 1 };
            }
        }
        // outside the text: the nearest end
        let (mut lo_i, mut hi_i) = (0usize, 0usize);
        for (i, e) in self.ext.iter().enumerate() {
            if e.0 < self.ext[lo_i].0 {
                lo_i = i;
            }
            if e.1 > self.ext[hi_i].1 {
                hi_i = i;
            }
        }
        if x < self.ext[lo_i].0 {
            if self.ext[lo_i].2 { lo_i + 1 } else { lo_i }
        } else if self.ext[hi_i].2 {
            hi_i
        } else {
            hi_i + 1
        }
    }

    /// Highlight rectangles (left, right) for the chars a..b, merged where they touch.
    pub fn sel_rects(&self, a: usize, b: usize) -> Vec<(f32, f32)> {
        let mut v: Vec<(f32, f32)> = (a..b.min(self.ext.len())).map(|i| (self.ext[i].0, self.ext[i].1)).collect();
        v.sort_by(|p, q| p.0.partial_cmp(&q.0).unwrap());
        let mut out: Vec<(f32, f32)> = Vec::new();
        for (l, r) in v {
            match out.last_mut() {
                Some(last) if l <= last.1 + 0.6 => last.1 = last.1.max(r),
                _ => out.push((l, r)),
            }
        }
        out
    }

    /// Caret index one step visually left or right of `i` (None at the line's edge).
    pub fn step(&self, i: usize, left: bool) -> Option<usize> {
        let cur = self.caret_x(i);
        let mut best: Option<(usize, f32)> = None;
        for k in 0..=self.ext.len() {
            let x = self.caret_x(k);
            let ok = if left { x < cur - 0.5 } else { x > cur + 0.5 };
            if ok {
                let d = (x - cur).abs();
                if best.map_or(true, |(_, bd)| d < bd - 0.01 || ((d - bd).abs() <= 0.01 && k.abs_diff(i) < best.unwrap().0.abs_diff(i))) {
                    best = Some((k, d));
                }
            }
        }
        best.map(|b| b.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AR: &str = "مرحبا بالعالم"; // "hello world"
    const FILE: &str = "تقرير المبيعات.pdf"; // "sales report.pdf"

    #[test]
    fn arabic_is_shaped_not_drawn_letter_by_letter() {
        let mut s = Shaper::new();
        // lam + alef joins into one ligature glyph
        let l = s.layout("لا", 20.0, Mode::Auto);
        assert_eq!(l.glyphs.len(), 1, "lam-alef is one glyph");
        assert!(l.rtl);
        // every char has an extent and the two chars of the ligature split it
        assert_eq!(l.ext.len(), 2);
        assert!(l.ext[0].1 <= l.ext[1].0 + 0.01 || l.ext[1].1 <= l.ext[0].0 + 0.01);
        // joined text: glyph ids differ from the isolated forms of the same letters
        let joined = s.layout("بب", 20.0, Mode::Auto);
        let sep = s.layout("ب ب", 20.0, Mode::Auto);
        assert_ne!(joined.glyphs[0].gid, sep.glyphs[0].gid, "initial form differs from isolated form");
        // no missing-glyph boxes for the whole sample phrase
        let l = s.layout(AR, 20.0, Mode::Auto);
        assert!(l.glyphs.iter().all(|g| g.gid != 0), "notdef in {AR}");
        assert!(l.width > 20.0);
    }

    #[test]
    fn every_char_has_a_sane_extent_and_carets_are_reversible() {
        let mut s = Shaper::new();
        for t in [AR, FILE, "abc ", "Report تقرير final", "12 ديسمبر 2026", ""] {
            let l = s.layout(t, 16.0, Mode::Auto);
            let n = t.chars().count();
            assert_eq!(l.ext.len(), n);
            for (i, e) in l.ext.iter().enumerate() {
                assert!(e.0 >= -0.01 && e.1 <= l.width + 0.01 && e.1 >= e.0, "{t:?} char {i}: {e:?} width {}", l.width);
            }
            // a click on the caret position of every index returns that index (or an equal-x neighbour)
            for i in 0..=n {
                let x = l.caret_x(i);
                let back = l.index_at(x + if i < n && !l.ext[i].2 { 0.2 } else if i < n { -0.2 } else { 0.0 });
                // at a direction boundary one x position is the end of one run and the start of the next:
                // either logical neighbour is a correct answer
                let trailing = |k: usize| if k == 0 { f32::NAN } else { let e = l.ext[k - 1]; if e.2 { e.0 } else { e.1 } };
                assert!((l.caret_x(back) - x).abs() < 0.6 || (trailing(back) - x).abs() < 0.6, "{t:?} i={i} back={back}");
            }
        }
    }

    #[test]
    fn mixed_text_places_latin_inside_arabic_in_the_right_visual_order() {
        let mut s = Shaper::new();
        let l = s.layout("افتح Downloads الآن", 16.0, Mode::Auto);
        assert!(l.rtl, "an Arabic sentence is a right-to-left paragraph");
        let pos = |ch: char| l.ext[ "افتح Downloads الآن".chars().position(|c| c == ch).unwrap() ];
        // first word (alef) is rightmost, Latin D is left of it, last word's first letter (alef of الآن) is left of the Latin
        let first_word = pos('ا').0;
        let latin = pos('D').0;
        assert!(latin < first_word, "Latin sits left of the first Arabic word");
        // Latin letters read left to right
        assert!(pos('D').0 < pos('s').0);
        // the last Arabic word is leftmost of all
        let last = l.ext[l.ext.len() - 1];
        assert!(last.0 < latin);
        // an English sentence stays left-to-right
        let e = s.layout("open Downloads", 16.0, Mode::Auto);
        assert!(!e.rtl);
    }

    #[test]
    fn strict_mode_keeps_path_parts_in_written_order_even_with_arabic_names() {
        let mut s = Shaper::new();
        let path = "~/Documents/تقرير/ملف.txt";
        let strict = s.layout(path, 16.0, Mode::Strict);
        assert!(!strict.rtl);
        let chars: Vec<char> = path.chars().collect();
        let slashes: Vec<usize> = chars.iter().enumerate().filter(|(_, c)| **c == '/').map(|(i, _)| i).collect();
        // slashes and the parts between them progress strictly left to right
        let xs: Vec<f32> = slashes.iter().map(|i| strict.ext[*i].0).collect();
        assert!(xs.windows(2).all(|w| w[0] < w[1]), "{xs:?}");
        let first_ar = chars.iter().position(|c| *c == 'ت').unwrap();
        let second_ar = chars.iter().position(|c| *c == 'م').unwrap();
        assert!(strict.ext[first_ar].0 >= strict.ext[slashes[1]].1 - 0.01, "first Arabic folder starts after its slash");
        assert!(strict.ext[first_ar].0 < strict.ext[second_ar].0);
        // without the segment split, Auto mode on the same text would flip the Arabic parts around the slash
        let auto = s.layout("تقرير/ملف", 16.0, Mode::Auto);
        let strict2 = s.layout("تقرير/ملف", 16.0, Mode::Strict);
        let slash = "تقرير/ملف".chars().position(|c| c == '/').unwrap();
        assert!(auto.ext[slash].0 < auto.ext[0].0, "auto: the slash reads right-to-left, the first folder is on the right");
        assert!(strict2.ext[0].0 < strict2.ext[slash].0, "strict: first folder is left of the slash");
        let first_folder_x = strict2.ext[0].0;
        let second_folder_x = strict2.ext[slash + 1].0;
        assert!(first_folder_x < second_folder_x, "strict: first folder is left of the second");
    }

    #[test]
    fn bidi_controls_cannot_reorder_anything_and_are_shown() {
        let mut s = Shaper::new();
        // right-to-left override that would make "txt.exe" look like "exe.txt" style tricks
        let evil = "report\u{202E}fdp.exe";
        assert!(has_bidi_control(evil));
        let l = s.layout(evil, 16.0, Mode::Strict);
        assert_eq!(l.ext.len(), evil.chars().count());
        // Latin letters keep reading left to right and the control is a visible marker glyph
        let idx = |c: char| evil.chars().position(|x| x == c).unwrap();
        let lastp = evil.chars().count() - 1 - evil.chars().rev().position(|x| x == 'p').unwrap();
        assert!(l.ext[idx('f')].0 < l.ext[idx('d')].0 && l.ext[idx('d')].0 < l.ext[lastp].0);
        let ctl = idx('\u{202E}');
        assert!(l.ext[ctl].1 > l.ext[ctl].0, "the marker takes space");
        let ar = s.layout("\u{202E}abc", 16.0, Mode::Auto);
        assert!(!ar.rtl, "an override cannot make a paragraph right-to-left");
    }

    #[test]
    fn visual_stepping_follows_what_the_user_sees() {
        let mut s = Shaper::new();
        let l = s.layout("ابج", 20.0, Mode::Auto); // three Arabic letters, read right to left
        // caret at the start sits on the right edge; Left moves visually left = logical forward
        assert_eq!(l.step(0, true), Some(1));
        assert_eq!(l.step(1, true), Some(2));
        assert_eq!(l.step(3, true), None);
        assert_eq!(l.step(3, false), Some(2));
        let e = s.layout("abc", 20.0, Mode::Auto);
        assert_eq!(e.step(0, false), Some(1));
        assert_eq!(e.step(0, true), None);
    }

    #[test]
    fn selection_in_mixed_text_is_visually_split_when_the_logical_range_is() {
        let mut s = Shaper::new();
        let t = "ab ابج cd";
        let l = s.layout(t, 16.0, Mode::Auto);
        let r = l.sel_rects(1, 8); // b, space, Arabic, space, c
        assert!(!r.is_empty());
        // a contiguous logical range inside one direction is one rectangle
        let r1 = l.sel_rects(3, 6);
        assert_eq!(r1.len(), 1);
    }
}
