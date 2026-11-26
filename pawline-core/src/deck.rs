//! "make a presentation from <outline file>": a plain outline (Markdown-style headings and bullets) becomes a .pptx.
//! Local only: no network, no AI, nothing is sent anywhere. The result is a NEW file next to the outline.
use std::path::{Path, PathBuf};

pub const MAX_SLIDES: usize = 60;
pub const MAX_BULLETS: usize = 14;
/// A slide keeps at most this many lines and characters; denser text continues on the next slide.
pub const SLIDE_LINES: usize = 6;
pub const SLIDE_CHARS: usize = 420;
/// Lines longer than this are cut at sentence ends; a single sentence longer than the hard limit is refused.
pub const SOFT_LINE: usize = 160;
pub const HARD_LINE: usize = 320;
pub const MAX_TITLE_CHARS: usize = 80;
pub const MAX_LINE_CHARS: usize = 160;
pub const MAX_SLIDE_CHARS: usize = 900;
pub const MAX_SOURCE: u64 = 256 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct Slide {
    pub title: String,
    /// (level 0 or 1, text)
    pub bullets: Vec<(u8, String)>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// The source had no "#" headings, so slides were made from its paragraphs.
    pub from_prose: bool,
    /// Extra slides made because the text was too dense for one slide.
    pub continuation: usize,
    /// Long lines cut into several bullets at sentence ends.
    pub lines_cut: usize,
    pub agenda: bool,
}

impl Report {
    pub fn describe(&self) -> String {
        let mut v = vec![];
        if self.from_prose {
            v.push("no headings found: one slide per paragraph, titles taken from each paragraph's first words".to_string());
        }
        if self.continuation > 0 {
            v.push(format!("{} dense slide(s) continued on a next slide", self.continuation));
        }
        if self.lines_cut > 0 {
            v.push(format!("{} long line(s) cut into bullets at sentence ends", self.lines_cut));
        }
        if self.agenda {
            v.push("a Contents slide added".to_string());
        }
        if v.is_empty() { "your outline was used as written".into() } else { v.join("; ") }
    }
}

/// Source text -> readable slides, locally and deterministically (no AI, no rewriting: every word of the source is
/// kept, only split, grouped and given titles). Outlines with "#" headings are used as written; plain paragraphs get
/// one slide each; dense slides continue on a next slide instead of being refused or shrunk to unreadable text.
pub fn compose(text: &str) -> Result<(Vec<Slide>, Report), String> {
    let has_headings = text.lines().any(|l| l.starts_with("# ") || l.starts_with("## ") || l.starts_with("### "));
    let first_is_title = text.lines().find(|l| !l.trim().is_empty()).is_some_and(|l| l.starts_with("# ")) || !has_headings;
    let mut rep = Report::default();
    let raw = if has_headings { parse_outline(text)? } else { rep.from_prose = true; parse_prose(text)? };
    let mut out: Vec<Slide> = vec![];
    for (i, s) in raw.into_iter().enumerate() {
        if i == 0 && s.bullets.len() <= 2 && first_is_title {
            out.push(s);
            continue;
        }
        let mut groups: Vec<Vec<(u8, String)>> = vec![];
        for (lvl, b) in s.bullets {
            let parts = if lvl == 0 && b.chars().count() > SOFT_LINE { split_sentences(&b)? } else { vec![b] };
            if parts.len() > 1 {
                rep.lines_cut += 1;
            }
            for (k, p) in parts.into_iter().enumerate() {
                let _ = k;
                if lvl == 1 {
                    match groups.last_mut() {
                        Some(g) => g.push((1, p)),
                        None => groups.push(vec![(1, p)]),
                    }
                } else {
                    groups.push(vec![(0, p)]);
                }
            }
        }
        let mut pages: Vec<Vec<(u8, String)>> = vec![vec![]];
        for g in groups {
            let cur = pages.last().unwrap();
            let chars: usize = cur.iter().map(|(_, b)| b.chars().count()).sum::<usize>() + g.iter().map(|(_, b)| b.chars().count()).sum::<usize>();
            if !cur.is_empty() && (cur.len() + g.len() > SLIDE_LINES || chars > SLIDE_CHARS) {
                pages.push(vec![]);
            }
            pages.last_mut().unwrap().extend(g);
        }
        let n = pages.len();
        for (k, p) in pages.into_iter().enumerate() {
            let title = if n > 1 { format!("{} ({}/{n})", s.title, k + 1) } else { s.title.clone() };
            if k > 0 {
                rep.continuation += 1;
            }
            if title.chars().count() > MAX_TITLE_CHARS {
                return Err(format!("the title \"{}...\" is too long once \"({}/{n})\" is added; shorten it", s.title.chars().take(30).collect::<String>(), k + 1));
            }
            out.push(Slide { title, bullets: p });
        }
    }
    // A Contents slide for longer talks: the original slide titles only (not continuations), after the title slide.
    let content_titles: Vec<String> = out.iter().skip(1).filter(|s| !is_continuation(&s.title)).map(|s| s.title.clone()).collect();
    if out.len() >= 5 && content_titles.len() >= 4 && content_titles.len() <= SLIDE_LINES + 2 {
        out.insert(1, Slide { title: "Contents".into(), bullets: content_titles.into_iter().map(|t| (0, t)).collect() });
        rep.agenda = true;
    }
    if out.len() > MAX_SLIDES {
        return Err(format!("that is {} slides after splitting dense ones; the limit is {MAX_SLIDES}", out.len()));
    }
    Ok((out, rep))
}

fn is_continuation(t: &str) -> bool {
    t.ends_with(')') && t.rfind(" (").is_some_and(|i| t[i + 2..t.len() - 1].split_once('/').is_some_and(|(a, b)| a.parse::<usize>().is_ok() && b.parse::<usize>().is_ok()))
}

/// Cut at sentence ends and re-pack up to SOFT_LINE. A sentence over HARD_LINE has no safe place to cut: refuse.
fn split_sentences(line: &str) -> Result<Vec<String>, String> {
    split_sent(line, true)
}

fn split_sent(line: &str, repack: bool) -> Result<Vec<String>, String> {
    let mut sents: Vec<String> = vec![];
    let mut cur = String::new();
    let cs: Vec<char> = line.chars().collect();
    for (i, c) in cs.iter().enumerate() {
        cur.push(*c);
        if matches!(c, '.' | '!' | '?' | '\u{061F}' | '\u{3002}') && cs.get(i + 1).is_none_or(|n| n.is_whitespace()) {
            sents.push(cur.trim().to_string());
            cur.clear();
        }
    }
    if !cur.trim().is_empty() {
        sents.push(cur.trim().to_string());
    }
    if let Some(l) = sents.iter().find(|s| s.chars().count() > HARD_LINE) {
        return Err(format!("a sentence starting \"{}...\" is longer than {HARD_LINE} characters with no place to cut it. Shorten it or split it", l.chars().take(30).collect::<String>()));
    }
    if !repack {
        return Ok(sents);
    }
    let mut out: Vec<String> = vec![];
    for s in sents {
        match out.last_mut() {
            Some(l) if l.chars().count() + 1 + s.chars().count() <= SOFT_LINE => {
                l.push(' ');
                l.push_str(&s);
            }
            _ => out.push(s),
        }
    }
    Ok(out)
}

/// Plain notes without headings: the first short line is the title (else "Presentation"); each blank-line-separated
/// paragraph is a slide whose title is its first words and whose bullets are its sentences (or lines).
fn parse_prose(text: &str) -> Result<Vec<Slide>, String> {
    let paras: Vec<Vec<&str>> = text
        .split("\n\n")
        .map(|p| p.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>())
        .filter(|p| !p.is_empty())
        .collect();
    if paras.is_empty() {
        return Err("I found no headings. Use \"# Title\" and \"## Slide title\" lines with \"- bullet\" lines under them, or plain paragraphs".into());
    }
    if paras.len() < 2 {
        return Err("that is too short to make slides from. Write a title line and a few paragraphs, or use \"# Title\" and \"## Slide title\" headings with \"- bullet\" lines".into());
    }
    let mut slides: Vec<Slide> = vec![];
    let mut it = paras.into_iter().peekable();
    let first = it.peek().unwrap();
    let ends = |l: &str| l.trim_end().ends_with(['.', '!', '?', ':', '\u{061F}']);
    if first.len() == 1 && first[0].chars().count() <= MAX_TITLE_CHARS && !ends(first[0]) {
        slides.push(Slide { title: first[0].to_string(), bullets: vec![] });
        it.next();
    } else {
        slides.push(Slide { title: "Presentation".into(), bullets: vec![] });
    }
    for p in it {
        let bullet_like = |l: &str| l.starts_with("- ") || l.starts_with("* ") || l.starts_with("+ ");
        let (title, rest): (String, Vec<(u8, String)>) = if p.len() > 1 && p[0].chars().count() <= 60 && !ends(p[0]) && !bullet_like(p[0]) {
            (p[0].to_string(), p[1..].iter().map(|l| (0u8, l.trim_start_matches(['-', '*', '+', ' ']).to_string())).collect())
        } else {
            let all = p.join(" ");
            let sents = split_sent(&all.replace(" - ", " "), false).unwrap_or_else(|_| vec![all.clone()]);
            let t = lead_words(&sents[0]);
            (t, sents.into_iter().map(|x| (0u8, x)).collect())
        };
        slides.push(Slide { title, bullets: rest });
    }
    Ok(slides)
}

/// Title from a paragraph's first words: up to 7 words, cut at a word boundary, no trailing punctuation.
fn lead_words(s: &str) -> String {
    let mut t = String::new();
    let mut more = false;
    for (i, w) in s.split_whitespace().enumerate() {
        if i >= 7 || (!t.is_empty() && t.chars().count() + 1 + w.chars().count() > 38) {
            more = true;
            break;
        }
        if !t.is_empty() {
            t.push(' ');
        }
        t.push_str(w);
    }
    let t = t.trim_end_matches([',', ';', ':', '.', '!', '?', '-']).to_string();
    if more { format!("{t}...") } else { t }
}

pub fn parse_command(input: &str) -> Option<String> {
    let t = input.trim().trim_end_matches(['.', '!']);
    let low = t.to_lowercase();
    for p in [
        "make a presentation from ", "make presentation from ", "create a presentation from ", "create presentation from ", "make slides from ",
        "make a slideshow from ", "make a deck from ", "presentation from ", "slides from ",
    ] {
        if low.starts_with(p) {
            let rest = t[p.len()..].trim();
            return if rest.is_empty() { None } else { Some(rest.to_string()) };
        }
    }
    None
}

/// Outline -> slides. `# Title` (first) is the title slide and may be followed by one plain subtitle line.
/// Each `## Heading` starts a slide. `- item` / `* item` are bullets (two or more leading spaces: a sub-bullet).
/// Plain lines under a slide are bullets too. Returns an error text the user can act on.
pub fn parse_outline(text: &str) -> Result<Vec<Slide>, String> {
    let mut slides: Vec<Slide> = vec![];
    let mut seen_title = false;
    for raw in text.lines() {
        let line = raw.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        if let Some(t) = line.strip_prefix("# ") {
            if !seen_title && slides.is_empty() {
                seen_title = true;
                slides.push(Slide { title: t.trim().to_string(), bullets: vec![] });
                continue;
            }
            slides.push(Slide { title: t.trim().to_string(), bullets: vec![] });
            continue;
        }
        if let Some(t) = line.strip_prefix("## ").or_else(|| line.strip_prefix("### ")) {
            slides.push(Slide { title: t.trim().to_string(), bullets: vec![] });
            continue;
        }
        let Some(cur) = slides.last_mut() else {
            return Err("the outline must start with a heading line like \"# My talk\" or \"## First slide\"".into());
        };
        let indent = line.len() - line.trim_start().len();
        let body = line.trim_start();
        let (is_b, body) = match body.strip_prefix("- ").or_else(|| body.strip_prefix("* ")).or_else(|| body.strip_prefix("+ ")) {
            Some(b) => (true, b),
            None => (false, body),
        };
        let level = if is_b && indent >= 2 { 1 } else { 0 };
        cur.bullets.push((level, body.trim().to_string()));
    }
    for s in &slides {
        let n = s.title.chars().count();
        if n > MAX_TITLE_CHARS {
            return Err(format!("the title \"{}...\" is {n} characters; the limit is {MAX_TITLE_CHARS}. Shorten it", s.title.chars().take(30).collect::<String>()));
        }
    }
    if slides.is_empty() {
        return Err("I found no headings. Use \"# Title\" and \"## Slide title\" lines with \"- bullet\" lines under them".into());
    }
    if slides.len() > MAX_SLIDES {
        return Err(format!("that is {} slides; the limit is {MAX_SLIDES}", slides.len()));
    }
    Ok(slides)
}

fn is_rtl_char(c: char) -> bool {
    matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF)
}

/// First strongly-directional letter decides the paragraph direction (right-to-left for Arabic/Hebrew).
fn is_rtl(s: &str) -> bool {
    s.chars().find(|c| c.is_alphabetic()).map(is_rtl_char).unwrap_or(false)
}

fn para_sz(lvl: u32, text: &str, sz: u32) -> String {
    let rtl = is_rtl(text);
    let ppr = match (lvl, rtl) {
        (0, false) => String::new(),
        (l, false) => format!("<a:pPr lvl=\"{l}\"/>"),
        (l, true) => format!("<a:pPr lvl=\"{l}\" algn=\"r\" rtl=\"1\"/>"),
    };
    let lang = if rtl { "ar-SA" } else { "en-US" };
    let sz = if lvl > 0 { sz * 5 / 6 } else { sz };
    format!("<a:p>{ppr}<a:r><a:rPr lang=\"{lang}\" sz=\"{sz}\" dirty=\"0\"/><a:t>{}</a:t></a:r></a:p>", esc(text))
}

/// Body text size: the largest size whose estimated wrapped lines fit the body box (about 780 x 330 points, average
/// character width 0.55 em, line height 1.2 em, 6 pt before each bullet; sub-bullets use 5/6 of the size). An estimate,
/// not a measurement: a real viewer may wrap differently, and PowerPoint/LibreOffice autofit is on as a second guard.
fn body_size(bullets: &[(u8, String)]) -> u32 {
    for sz in [3200u32, 2800, 2400, 2200, 2000, 1800] {
        let pt = sz as f32 / 100.0;
        let mut h = 0.0;
        for (l, b) in bullets {
            let p = if *l > 0 { pt * 5.0 / 6.0 } else { pt };
            let width = if *l > 0 { 740.0 } else { 780.0 };
            let per_line = (width / (p * 0.55)).floor().max(1.0);
            let lines = (b.chars().count() as f32 / per_line).ceil().max(1.0);
            h += lines * p * 1.2 + 6.0;
        }
        if h <= 330.0 {
            return sz;
        }
    }
    1800
}

fn para(lvl: u32, text: &str) -> String {
    let rtl = is_rtl(text);
    let ppr = match (lvl, rtl) {
        (0, false) => String::new(),
        (l, false) => format!("<a:pPr lvl=\"{l}\"/>"),
        (l, true) => format!("<a:pPr lvl=\"{l}\" algn=\"r\" rtl=\"1\"/>"),
    };
    let lang = if rtl { "ar-SA" } else { "en-US" };
    format!("<a:p>{ppr}<a:r><a:rPr lang=\"{lang}\" dirty=\"0\"/><a:t>{}</a:t></a:r></a:p>", esc(text))
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            c if (c as u32) < 0x20 && c != '\t' => {}
            c => o.push(c),
        }
    }
    o
}

const NS: &str = "xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"";
const HDR: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";

fn sp(id: u32, name: &str, ph: &str, idx: Option<u32>, x: i64, y: i64, w: i64, h: i64, body: &str) -> String {
    let idxa = idx.map(|i| format!(" idx=\"{i}\"")).unwrap_or_default();
    format!("<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{name}\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"{ph}\"{idxa}/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{w}\" cy=\"{h}\"/></a:xfrm></p:spPr>{body}</p:sp>")
}

fn slide_xml(s: &Slide, is_title: bool) -> String {
    let mut shapes = String::new();
    let t = format!("<p:txBody><a:bodyPr/><a:lstStyle/>{}</p:txBody>", para(0, &s.title));
    if is_title {
        shapes.push_str(&format!("<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Title\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"ctrTitle\"/></p:nvPr></p:nvSpPr><p:spPr/>{t}</p:sp>"));
        if !s.bullets.is_empty() {
            let paras: String = s.bullets.iter().map(|(_, b)| para(0, b)).collect();
            shapes.push_str(&format!("<p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Subtitle\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"subTitle\" idx=\"1\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/>{paras}</p:txBody></p:sp>"));
        }
    } else {
        shapes.push_str(&format!("<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Title\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"title\"/></p:nvPr></p:nvSpPr><p:spPr/>{t}</p:sp>"));
        let paras: String = if s.bullets.is_empty() {
            "<a:p><a:endParaRPr lang=\"en-US\"/></a:p>".into()
        } else {
            { let z = body_size(&s.bullets); s.bullets.iter().map(|(l, b)| para_sz(*l as u32, b, z)).collect() }
        };
        shapes.push_str(&format!("<p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Content\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph idx=\"1\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr><a:noAutofit/></a:bodyPr><a:lstStyle/>{paras}</p:txBody></p:sp>"));
    }
    format!("{HDR}<p:sld {NS}><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{shapes}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>")
}

fn layout_xml(name: &str, ty: &str, title_ph: &str, body_ph: Option<(&str, &str)>) -> String {
    let mut shapes = format!("<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Title\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"{title_ph}\"/></p:nvPr></p:nvSpPr><p:spPr>{}</p:spPr><p:txBody><a:bodyPr/>{}<a:p><a:r><a:rPr lang=\"en-US\"/><a:t>Title</a:t></a:r></a:p></p:txBody></p:sp>",
        if ty == "title" { "<a:xfrm><a:off x=\"1524000\" y=\"1900000\"/><a:ext cx=\"9144000\" cy=\"1600000\"/></a:xfrm>" } else { "" },
        if ty == "title" { "<a:lstStyle><a:lvl1pPr><a:defRPr sz=\"5000\" b=\"1\"/></a:lvl1pPr></a:lstStyle>" } else { "<a:lstStyle/>" });
    if let Some((bt, bi)) = body_ph {
        let lst = if ty == "title" { "<a:lstStyle><a:lvl1pPr marL=\"0\" indent=\"0\"><a:buNone/><a:defRPr sz=\"2400\"/></a:lvl1pPr></a:lstStyle>" } else { "<a:lstStyle/>" };
        let geom = if ty == "title" { "<a:xfrm><a:off x=\"1524000\" y=\"3700000\"/><a:ext cx=\"9144000\" cy=\"1200000\"/></a:xfrm>" } else { "" };
        shapes.push_str(&format!("<p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Body\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph {bt}idx=\"{bi}\"/></p:nvPr></p:nvSpPr><p:spPr>{geom}</p:spPr><p:txBody><a:bodyPr/>{lst}<a:p><a:r><a:rPr lang=\"en-US\"/><a:t>Text</a:t></a:r></a:p></p:txBody></p:sp>"));
    }
    format!("{HDR}<p:sldLayout {NS} type=\"{ty}\" preserve=\"1\"><p:cSld name=\"{name}\"><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{shapes}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>")
}

fn master_xml() -> String {
    let title = sp(2, "Title", "title", None, 838200, 365125, 10515600, 1325563, "<p:txBody><a:bodyPr anchor=\"ctr\"><a:normAutofit/></a:bodyPr><a:lstStyle/><a:p><a:r><a:rPr lang=\"en-US\"/><a:t>Title</a:t></a:r></a:p></p:txBody>");
    let body = "<p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Body\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"body\" idx=\"1\"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"838200\" y=\"1825625\"/><a:ext cx=\"10515600\" cy=\"4351338\"/></a:xfrm></p:spPr><p:txBody><a:bodyPr><a:noAutofit/></a:bodyPr><a:lstStyle/><a:p><a:r><a:rPr lang=\"en-US\"/><a:t>Text</a:t></a:r></a:p></p:txBody></p:sp>";
    let lvl = |n: u32, marl: u32, sz: u32| format!("<a:lvl{n}pPr marL=\"{marl}\" indent=\"-285750\"><a:spcBef><a:spcPts val=\"600\"/></a:spcBef><a:buFont typeface=\"Arial\"/><a:buChar char=\"&#8226;\"/><a:defRPr sz=\"{sz}\" kern=\"1200\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mn-lt\"/></a:defRPr></a:lvl{n}pPr>");
    format!("{HDR}<p:sldMaster {NS}><p:cSld><p:bg><p:bgRef idx=\"1001\"><a:schemeClr val=\"bg1\"/></p:bgRef></p:bg><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{title}{body}</p:spTree></p:cSld><p:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"/><p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"rId1\"/><p:sldLayoutId id=\"2147483650\" r:id=\"rId2\"/></p:sldLayoutIdLst><p:txStyles><p:titleStyle><a:lvl1pPr algn=\"l\"><a:defRPr sz=\"3600\" b=\"1\" kern=\"1200\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mj-lt\"/></a:defRPr></a:lvl1pPr></p:titleStyle><p:bodyStyle>{}{}</p:bodyStyle><p:otherStyle><a:lvl1pPr><a:defRPr sz=\"1800\"/></a:lvl1pPr></p:otherStyle></p:txStyles></p:sldMaster>", lvl(1, 342900, 2800), lvl(2, 800100, 2400))
}

fn theme_xml() -> String {
    format!("{HDR}<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"Pawline\"><a:themeElements><a:clrScheme name=\"Pawline\"><a:dk1><a:srgbClr val=\"1F1D2B\"/></a:dk1><a:lt1><a:srgbClr val=\"FFFFFF\"/></a:lt1><a:dk2><a:srgbClr val=\"3B3558\"/></a:dk2><a:lt2><a:srgbClr val=\"EEEBF5\"/></a:lt2><a:accent1><a:srgbClr val=\"7C5CD6\"/></a:accent1><a:accent2><a:srgbClr val=\"E8828C\"/></a:accent2><a:accent3><a:srgbClr val=\"4E9F7D\"/></a:accent3><a:accent4><a:srgbClr val=\"D9A441\"/></a:accent4><a:accent5><a:srgbClr val=\"4A8FC7\"/></a:accent5><a:accent6><a:srgbClr val=\"8A8AA0\"/></a:accent6><a:hlink><a:srgbClr val=\"4A8FC7\"/></a:hlink><a:folHlink><a:srgbClr val=\"7C5CD6\"/></a:folHlink></a:clrScheme><a:fontScheme name=\"Pawline\"><a:majorFont><a:latin typeface=\"DejaVu Sans\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont><a:minorFont><a:latin typeface=\"DejaVu Sans\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:minorFont></a:fontScheme><a:fmtScheme name=\"Pawline\"><a:fillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w=\"9525\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln><a:ln w=\"19050\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln><a:ln w=\"28575\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>")
}

const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn rels(items: &[(String, &str, String)]) -> String {
    let mut s = format!("{HDR}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">");
    for (id, ty, target) in items {
        s.push_str(&format!("<Relationship Id=\"{id}\" Type=\"{REL}/{ty}\" Target=\"{target}\"/>"));
    }
    s.push_str("</Relationships>");
    s
}

/// The complete .pptx file as bytes.
pub fn build_pptx(slides: &[Slide]) -> Vec<u8> {
    let mut files: Vec<(String, Vec<u8>)> = vec![];
    let mut ct = format!("{HDR}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/><Override PartName=\"/ppt/slideMasters/slideMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/><Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/><Override PartName=\"/ppt/slideLayouts/slideLayout2.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/><Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>");
    for i in 1..=slides.len() {
        ct.push_str(&format!("<Override PartName=\"/ppt/slides/slide{i}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"));
    }
    ct.push_str("</Types>");
    files.push(("[Content_Types].xml".into(), ct.into_bytes()));
    files.push(("_rels/.rels".into(), rels(&[("rId1".into(), "officeDocument", "ppt/presentation.xml".into())]).into_bytes()));
    let mut prel: Vec<(String, &str, String)> = vec![("rId1".into(), "slideMaster", "slideMasters/slideMaster1.xml".into()), ("rId2".into(), "theme", "theme/theme1.xml".into())];
    let mut ids = String::new();
    for i in 1..=slides.len() {
        prel.push((format!("rId{}", i + 2), "slide", format!("slides/slide{i}.xml")));
        ids.push_str(&format!("<p:sldId id=\"{}\" r:id=\"rId{}\"/>", 255 + i, i + 2));
    }
    files.push(("ppt/_rels/presentation.xml.rels".into(), rels(&prel).into_bytes()));
    files.push(("ppt/presentation.xml".into(), format!("{HDR}<p:presentation {NS}><p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rId1\"/></p:sldMasterIdLst><p:sldIdLst>{ids}</p:sldIdLst><p:sldSz cx=\"12192000\" cy=\"6858000\"/><p:notesSz cx=\"6858000\" cy=\"9144000\"/></p:presentation>").into_bytes()));
    files.push(("ppt/slideMasters/slideMaster1.xml".into(), master_xml().into_bytes()));
    files.push(("ppt/slideMasters/_rels/slideMaster1.xml.rels".into(), rels(&[("rId1".into(), "slideLayout", "../slideLayouts/slideLayout1.xml".into()), ("rId2".into(), "slideLayout", "../slideLayouts/slideLayout2.xml".into()), ("rId3".into(), "theme", "../theme/theme1.xml".into())]).into_bytes()));
    files.push(("ppt/slideLayouts/slideLayout1.xml".into(), layout_xml("Title Slide", "title", "ctrTitle", Some(("type=\"subTitle\" ", "1"))).into_bytes()));
    files.push(("ppt/slideLayouts/slideLayout2.xml".into(), layout_xml("Title and Content", "obj", "title", Some(("", "1"))).into_bytes()));
    for l in 1..=2 {
        files.push((format!("ppt/slideLayouts/_rels/slideLayout{l}.xml.rels"), rels(&[("rId1".into(), "slideMaster", "../slideMasters/slideMaster1.xml".into())]).into_bytes()));
    }
    files.push(("ppt/theme/theme1.xml".into(), theme_xml().into_bytes()));
    for (i, s) in slides.iter().enumerate() {
        let n = i + 1;
        let is_title = i == 0 && s.title_slide_like();
        files.push((format!("ppt/slides/slide{n}.xml"), slide_xml(s, is_title).into_bytes()));
        let layout = if is_title { 1 } else { 2 };
        files.push((format!("ppt/slides/_rels/slide{n}.xml.rels"), rels(&[("rId1".into(), "slideLayout", format!("../slideLayouts/slideLayout{layout}.xml"))]).into_bytes()));
    }
    zip_stored(&files)
}

impl Slide {
    /// The first slide is a title slide when it has at most two plain lines (a subtitle) and came from `# Title`.
    fn title_slide_like(&self) -> bool {
        self.bullets.len() <= 2
    }
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    let mut c = 0xFFFF_FFFFu32;
    for b in data {
        c = table[((c ^ *b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

/// Zip with stored (uncompressed) entries and a fixed 1980-01-01 timestamp, so the same outline gives the same bytes.
fn zip_stored(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let off = out.len() as u32;
        let crc = crc32(data);
        let nb = name.as_bytes();
        let (t, d) = (0u16, 0x0021u16);
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        for v in [20u16, 0x0800, 0, t, d] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(nb.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(nb);
        out.extend_from_slice(data);
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        for v in [20u16, 20, 0x0800, 0, t, d] {
            central.extend_from_slice(&v.to_le_bytes());
        }
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(nb.len() as u16).to_le_bytes());
        for v in [0u16, 0, 0] {
            central.extend_from_slice(&v.to_le_bytes());
        }
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&off.to_le_bytes());
        central.extend_from_slice(nb);
    }
    let cd_off = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    for v in [0u16, 0, files.len() as u16, files.len() as u16] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// Candidate outline files (.md, .txt) for the words: an explicit path, or a name in a few well-known folders.
pub fn find(home: &Path, words: &str) -> Vec<PathBuf> {
    let w = words.trim().trim_matches(|c| c == '"' || c == '`');
    let ok = |p: &Path| p.extension().map(|e| matches!(e.to_string_lossy().to_lowercase().as_str(), "md" | "txt" | "markdown")).unwrap_or(false);
    let direct = if let Some(rest) = w.strip_prefix("~/") { Some(home.join(rest)) } else if w.starts_with('/') { Some(PathBuf::from(w)) } else { None };
    if let Some(p) = direct {
        return match std::fs::symlink_metadata(&p) {
            Ok(m) if m.is_file() && ok(&p) => std::fs::canonicalize(&p).ok().filter(|c| c.starts_with(home)).into_iter().collect(),
            _ => vec![],
        };
    }
    let want: Vec<String> = w.to_lowercase().split_whitespace().map(String::from).collect();
    if want.is_empty() {
        return vec![];
    }
    let mut out = vec![];
    for d in ["Documents", "Desktop", "Downloads", ""] {
        let dir = if d.is_empty() { home.to_path_buf() } else { home.join(d) };
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            if !e.file_type().map(|t| t.is_file() && !t.is_symlink()).unwrap_or(false) {
                continue;
            }
            let p = e.path();
            let n = p.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
            let mut at = 0;
            let hit = ok(&p) && want.iter().all(|k| match n[at..].find(k.as_str()) { Some(i) => { at += i + k.len(); true } None => false });
            if hit {
                out.push(p);
            }
        }
    }
    out.sort();
    out.dedup();
    let exact: Vec<PathBuf> = out.iter().filter(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase() == w.to_lowercase()).unwrap_or(false)).cloned().collect();
    if exact.len() == 1 { exact } else { out }
}

/// `<stem>.pptx` next to the outline; `<stem>-2.pptx`, ... when taken.
pub fn output_for(from: &Path) -> PathBuf {
    let dir = from.parent().unwrap_or(Path::new("/"));
    let stem = from.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "slides".into());
    let mut n = 1;
    loop {
        let name = if n == 1 { format!("{stem}.pptx") } else { format!("{stem}-{n}.pptx") };
        let p = dir.join(name);
        if std::fs::symlink_metadata(&p).is_err() {
            return p;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn outline_parses_and_errors_are_actionable() {
        let s = parse_outline("# Talk\nby me\n\n## One\n- a\n  - sub\n- b\n## Two\nplain line\n").unwrap();
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].title, "Talk");
        assert_eq!(s[1].bullets, vec![(0, "a".to_string()), (1, "sub".to_string()), (0, "b".to_string())]);
        assert_eq!(s[2].bullets, vec![(0, "plain line".to_string())]);
        assert!(parse_outline("no headings here").unwrap_err().contains("heading"));
        assert!(parse_outline("").unwrap_err().contains("no headings"));
        assert_eq!(parse_command("make a presentation from notes.md").as_deref(), Some("notes.md"));
    }
    #[test]
    fn long_text_is_refused_not_clipped_and_rtl_is_marked() {
        let long = "x".repeat(MAX_TITLE_CHARS + 1);
        assert!(parse_outline(&format!("## {long}\n- a")).unwrap_err().contains("limit"));
        // a dense text is no longer refused: it is cut and continued (see compose tests); only uncuttable text is refused
        let line = "y".repeat(HARD_LINE + 1);
        assert!(compose(&format!("## T\n- {line}")).unwrap_err().contains("no place to cut"));
        let s = parse_outline("# مرحبا\n## الشرائح\n- نقطة أولى\n- English line\n").unwrap();
        let t = String::from_utf8_lossy(&build_pptx(&s)).to_string();
        assert!(t.contains("rtl=\"1\"") && t.contains("ar-SA") && t.contains("نقطة أولى"));
        assert_eq!(t.matches("rtl=\"1\"").count(), 3);
    }
    #[test]
    fn dense_outline_is_split_not_shrunk_and_keeps_every_word() {
        let bullets: String = (0..9).map(|i| format!("- point number {i} with some words to fill the line a little\n")).collect();
        let (s, rep) = compose(&format!("# Talk\n## Dense\n{bullets}## Next\n- ok\n")).unwrap();
        assert!(s.iter().all(|x| x.bullets.len() <= SLIDE_LINES));
        assert_eq!(s.iter().filter(|x| x.title.starts_with("Dense (")).count(), 2);
        assert!(s.iter().any(|x| x.title == "Dense (2/2)") && rep.continuation == 1);
        let words: usize = s.iter().flat_map(|x| x.bullets.iter()).map(|(_, b)| b.split_whitespace().count()).sum();
        assert_eq!(words, 9 * 12 + 1);
        // sub-bullets stay with their parent
        let (s, _) = compose("## T\n- a\n  - a1\n- b\n  - b1\n- c\n- d\n- e\n").unwrap();
        assert!(s.len() == 2 && s[0].bullets.last().unwrap().0 == 1 || s[1].bullets[0].0 == 0);
    }
    #[test]
    fn long_line_is_cut_at_sentences_and_one_giant_sentence_is_refused() {
        let line = "This is the first sentence of a long point that carries some detail. Here is a second sentence that adds more detail to it for the room. And a third one closes the thought for the audience today with a clear ending.";
        let (s, rep) = compose(&format!("## T\n- {line}\n")).unwrap();
        assert!(s[0].bullets.len() >= 2 && rep.lines_cut == 1);
        assert_eq!(s[0].bullets.iter().map(|(_, b)| b.as_str()).collect::<Vec<_>>().join(" "), line);
        assert!(compose(&format!("## T\n- {}\n", "word ".repeat(70))).is_err());
    }
    #[test]
    fn plain_paragraphs_become_slides_with_title_from_first_words() {
        let t = "My trip notes\n\nWe left early in the morning and drove north along the coast. The road was empty. We stopped twice for tea.\n\nDay two\nSwam in the sea\nWalked the old town\n";
        let (s, rep) = compose(t).unwrap();
        assert!(rep.from_prose);
        assert_eq!(s[0].title, "My trip notes");
        assert_eq!(s[1].title, "We left early in the morning and...");
        assert!(s[1].title.chars().count() <= 41);
        assert_eq!(s[1].bullets.len(), 3);
        assert_eq!(s[2].title, "Day two");
        assert_eq!(s[2].bullets.len(), 2);
    }
    #[test]
    fn contents_slide_only_for_longer_talks_and_lists_original_titles() {
        let body: String = (1..=5).map(|i| format!("## Part {i}\n- x\n")).collect();
        let (s, rep) = compose(&format!("# Talk\n{body}")).unwrap();
        assert!(rep.agenda && s[1].title == "Contents" && s[1].bullets.len() == 5);
        let (s, rep) = compose("# Talk\n## A\n- x\n## B\n- y\n").unwrap();
        assert!(!rep.agenda && s.len() == 3);
    }
    #[test]
    fn type_gets_smaller_as_the_slide_fills() {
        let few = vec![(0u8, "a".to_string())];
        let full: Vec<(u8, String)> = (0..6).map(|_| (0u8, "x".repeat(60))).collect();
        assert!(body_size(&few) > body_size(&full) && body_size(&full) >= 1800);
        // long bullets on a slide with room still get readable type
        let three_long: Vec<(u8, String)> = (0..3).map(|_| (0u8, "w".repeat(130))).collect();
        assert!(body_size(&three_long) >= 2400);
    }
    #[test]
    fn zip_is_deterministic_and_escaped() {
        let s = parse_outline("# A & <B>\n## x\n- \"q\"\n").unwrap();
        let a = build_pptx(&s);
        assert_eq!(a, build_pptx(&s));
        assert_eq!(&a[0..4], b"PK\x03\x04");
        let t = String::from_utf8_lossy(&a).to_string();
        assert!(t.contains("A &amp; &lt;B&gt;") && !t.contains("<B>"));
    }
    #[test]
    fn writes_a_sample_when_asked() {
        if let Some(p) = std::env::var_os("PAWLINE_DECK_SAMPLE") {
            if let Some(o) = std::env::var_os("PAWLINE_DECK_OUTLINE") {
                let (s, _) = compose(&std::fs::read_to_string(o).unwrap()).unwrap();
                std::fs::write(p, build_pptx(&s)).unwrap();
                return;
            }
            let s = parse_outline("# Pawline demo\nA small helper for your files\n\n## What it does\n- Makes folders and moves files\n  - always asks first\n- Shrinks videos\n- Searches with fd and rg\n\n## Safety\n- Nothing is overwritten\n- Undo for every change\n").unwrap();
            std::fs::write(p, build_pptx(&s)).unwrap();
        }
    }
}
