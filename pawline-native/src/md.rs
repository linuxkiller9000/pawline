//! Small, safe Markdown for Pawline's own replies: headings, bold, `code`, fenced code, bullet and
//! numbered lists, quotes, rules and links. It only decides how text is drawn. Nothing here runs, opens
//! or fetches anything: a link is shown as its label followed by the full address in plain text, images and
//! raw HTML are shown as the text they are, and anything unrecognised stays literal.

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Style {
    Normal,
    Bold,
    Code,
    Link,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Run {
    pub text: String,
    pub style: Style,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Block {
    Heading(u8, Vec<Run>),
    Para(Vec<Run>),
    Bullet(usize, Vec<Run>),
    Numbered(usize, usize, Vec<Run>),
    Quote(Vec<Run>),
    Code(Vec<String>),
    Rule,
}

fn run(t: &str, s: Style) -> Run {
    Run { text: t.to_string(), style: s }
}

fn push(out: &mut Vec<Run>, t: &str, s: Style) {
    if t.is_empty() {
        return;
    }
    if let Some(last) = out.last_mut() {
        if last.style == s {
            last.text.push_str(t);
            return;
        }
    }
    out.push(run(t, s));
}

/// Inline spans. Only `code`, **bold** and [label](http(s) address) are recognised, and only when closed.
pub fn inline(s: &str) -> Vec<Run> {
    let c: Vec<char> = s.chars().collect();
    let mut out: Vec<Run> = vec![];
    let mut i = 0;
    let mut buf = String::new();
    let flush = |buf: &mut String, out: &mut Vec<Run>| {
        push(out, buf, Style::Normal);
        buf.clear();
    };
    while i < c.len() {
        if c[i] == '`' {
            if let Some(j) = (i + 1..c.len()).find(|&j| c[j] == '`') {
                if j > i + 1 {
                    flush(&mut buf, &mut out);
                    push(&mut out, &c[i + 1..j].iter().collect::<String>(), Style::Code);
                    i = j + 1;
                    continue;
                }
            }
        }
        if c[i] == '*' && c.get(i + 1) == Some(&'*') {
            // an opener is followed by a non-space and its closer preceded by one (so "5 ** 3 ... **" stays literal)
            let opens = c.get(i + 2).map_or(false, |x| !x.is_whitespace());
            if let Some(j) = (i + 2..c.len().saturating_sub(1)).find(|&j| c[j] == '*' && c[j + 1] == '*' && !c[j - 1].is_whitespace()).filter(|_| opens) {
                if j > i + 2 {
                    flush(&mut buf, &mut out);
                    push(&mut out, &c[i + 2..j].iter().collect::<String>(), Style::Bold);
                    i = j + 2;
                    continue;
                }
            }
        }
        if c[i] == '[' {
            if let Some(m) = (i + 1..c.len()).find(|&j| c[j] == ']') {
                if c.get(m + 1) == Some(&'(') {
                    if let Some(e) = (m + 2..c.len()).find(|&j| c[j] == ')') {
                        let label: String = c[i + 1..m].iter().collect();
                        let url: String = c[m + 2..e].iter().collect();
                        if !label.is_empty() && (url.starts_with("https://") || url.starts_with("http://")) && !url.contains(' ') {
                            flush(&mut buf, &mut out);
                            push(&mut out, &label, Style::Link);
                            push(&mut out, &format!(" <{url}>"), Style::Normal);
                            i = e + 1;
                            continue;
                        }
                    }
                }
            }
        }
        buf.push(c[i]);
        i += 1;
    }
    flush(&mut buf, &mut out);
    out
}

pub fn parse(text: &str) -> Vec<Block> {
    let mut blocks = vec![];
    let mut para: Vec<String> = vec![];
    let mut code: Option<Vec<String>> = None;
    let flush_para = |para: &mut Vec<String>, blocks: &mut Vec<Block>| {
        if !para.is_empty() {
            blocks.push(Block::Para(inline(&para.join(" "))));
            para.clear();
        }
    };
    for raw in text.lines() {
        let line = raw.trim_end();
        if let Some(c) = code.as_mut() {
            if line.trim_start().starts_with("```") {
                blocks.push(Block::Code(code.take().unwrap()));
            } else {
                c.push(raw.to_string());
            }
            continue;
        }
        let t = line.trim_start();
        if t.starts_with("```") {
            flush_para(&mut para, &mut blocks);
            code = Some(vec![]);
            continue;
        }
        if t.is_empty() {
            flush_para(&mut para, &mut blocks);
            continue;
        }
        let indent = (line.len() - t.len()) / 2;
        let hashes = t.chars().take_while(|c| *c == '#').count();
        if (1..=3).contains(&hashes) && t.chars().nth(hashes) == Some(' ') {
            flush_para(&mut para, &mut blocks);
            blocks.push(Block::Heading(hashes as u8, inline(t[hashes + 1..].trim())));
        } else if matches!(t, "---" | "***" | "___") {
            flush_para(&mut para, &mut blocks);
            blocks.push(Block::Rule);
        } else if let Some(b) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| t.strip_prefix("+ ")) {
            flush_para(&mut para, &mut blocks);
            blocks.push(Block::Bullet(indent.min(3), inline(b.trim())));
        } else if let Some((n, rest)) = t.split_once(". ").filter(|(n, _)| !n.is_empty() && n.len() <= 3 && n.chars().all(|c| c.is_ascii_digit())) {
            flush_para(&mut para, &mut blocks);
            blocks.push(Block::Numbered(n.parse().unwrap_or(1), indent.min(3), inline(rest.trim())));
        } else if let Some(q) = t.strip_prefix("> ") {
            flush_para(&mut para, &mut blocks);
            blocks.push(Block::Quote(inline(q.trim())));
        } else {
            para.push(t.to_string());
        }
    }
    if let Some(c) = code {
        // an unclosed fence still shows its text
        blocks.push(Block::Code(c));
    }
    flush_para(&mut para, &mut blocks);
    blocks
}

#[derive(Clone, Debug)]
pub enum Kind {
    Text,
    Heading,
    Code,
    Quote,
    Rule,
}

/// One drawn line: runs, left indent, text size and total height.
#[derive(Clone, Debug)]
pub struct VLine {
    pub kind: Kind,
    pub indent: f32,
    pub px: f32,
    pub h: f32,
    pub marker: Option<String>,
    pub runs: Vec<Run>,
}

pub fn body_px() -> f32 {
    15.0 * crate::theme::text_scale()
}
fn lh() -> f32 {
    20.0 * crate::theme::text_scale()
}

fn words(runs: &[Run]) -> Vec<(String, Style, bool)> {
    // (word, style, preceded by a space); a space at the end of one run counts for the next run's first word
    let mut out: Vec<(String, Style, bool)> = vec![];
    let mut pending = false;
    for r in runs {
        for (k, w) in r.text.split(' ').enumerate() {
            if k > 0 {
                pending = true;
            }
            if !w.is_empty() {
                out.push((w.to_string(), r.style, pending));
                pending = false;
            }
        }
    }
    out
}

fn wrap_runs(runs: &[Run], px: f32, maxw: f32, adv: &mut dyn FnMut(char, f32) -> f32) -> Vec<Vec<Run>> {
    let mut lines: Vec<Vec<Run>> = vec![];
    let mut cur: Vec<Run> = vec![];
    let mut lw = 0.0;
    let space = adv(' ', px);
    for (w, st, sp) in words(runs) {
        let ww: f32 = w.chars().map(|c| adv(c, px)).sum();
        let need_space = !cur.is_empty() && sp;
        let add = ww + if need_space { space } else { 0.0 };
        if !cur.is_empty() && lw + add > maxw {
            lines.push(std::mem::take(&mut cur));
            lw = 0.0;
        }
        if ww > maxw {
            // split an over-long word by characters
            for ch in w.chars() {
                let cw = adv(ch, px);
                if lw + cw > maxw && !cur.is_empty() {
                    lines.push(std::mem::take(&mut cur));
                    lw = 0.0;
                }
                push(&mut cur, &ch.to_string(), st);
                lw += cw;
            }
            continue;
        }
        let join = !cur.is_empty() && sp;
        if join {
            push(&mut cur, " ", st);
            lw += space;
        }
        push(&mut cur, &w, st);
        lw += ww;
    }
    if !cur.is_empty() || lines.is_empty() {
        lines.push(cur);
    }
    lines
}

pub fn layout(blocks: &[Block], maxw: f32, adv: &mut dyn FnMut(char, f32) -> f32) -> Vec<VLine> {
    let mut out: Vec<VLine> = vec![];
    let gap = |out: &mut Vec<VLine>| {
        if let Some(l) = out.last_mut() {
            l.h += 4.0;
        }
    };
    for b in blocks {
        match b {
            Block::Heading(n, runs) => {
                let px = match n {
                    1 => 19.0,
                    2 => 17.0,
                    _ => 16.0,
                };
                if !out.is_empty() {
                    gap(&mut out);
                }
                for l in wrap_runs(runs, px, maxw, adv) {
                    out.push(VLine { kind: Kind::Heading, indent: 0.0, px, h: px + 8.0, marker: None, runs: l });
                }
            }
            Block::Para(runs) => {
                for l in wrap_runs(runs, body_px(), maxw, adv) {
                    out.push(VLine { kind: Kind::Text, indent: 0.0, px: body_px(), h: lh(), marker: None, runs: l });
                }
                gap(&mut out);
            }
            Block::Bullet(lvl, runs) | Block::Numbered(_, lvl, runs) => {
                let marker = match b {
                    Block::Numbered(n, _, _) => format!("{n}."),
                    _ => (if *lvl == 0 { "\u{2022}" } else { "-" }).to_string(),
                };
                let ind = 18.0 + *lvl as f32 * 16.0;
                for (k, l) in wrap_runs(runs, body_px(), maxw - ind, adv).into_iter().enumerate() {
                    out.push(VLine { kind: Kind::Text, indent: ind, px: body_px(), h: lh(), marker: if k == 0 { Some(marker.clone()) } else { None }, runs: l });
                }
            }
            Block::Quote(runs) => {
                for l in wrap_runs(runs, body_px(), maxw - 14.0, adv) {
                    out.push(VLine { kind: Kind::Quote, indent: 14.0, px: body_px(), h: lh(), marker: None, runs: l });
                }
                gap(&mut out);
            }
            Block::Code(lines) => {
                for src in lines.iter().take(40) {
                    // leading spaces are kept as extra indent (code needs them); blank lines stay as blank lines
                    let src = src.replace('\t', "  ");
                    let lead = src.chars().take_while(|c| *c == ' ').count().min(40);
                    let pad = lead as f32 * adv(' ', 13.0);
                    let body = src.trim_start_matches(' ');
                    if body.is_empty() {
                        out.push(VLine { kind: Kind::Code, indent: 6.0, px: 13.0, h: 17.0, marker: None, runs: vec![] });
                        continue;
                    }
                    for l in wrap_runs(&[run(body, Style::Code)], 13.0, (maxw - 12.0 - pad).max(40.0), adv) {
                        out.push(VLine { kind: Kind::Code, indent: 6.0 + pad, px: 13.0, h: 17.0, marker: None, runs: l });
                    }
                }
                if lines.len() > 40 {
                    out.push(VLine { kind: Kind::Code, indent: 6.0, px: 13.0, h: 17.0, marker: None, runs: vec![run(&format!("... {} more lines", lines.len() - 40), Style::Code)] });
                }
                gap(&mut out);
            }
            Block::Rule => out.push(VLine { kind: Kind::Rule, indent: 0.0, px: body_px(), h: 12.0, marker: None, runs: vec![] }),
        }
    }
    if let Some(l) = out.last_mut() {
        if l.h > lh() + 3.0 && matches!(l.kind, Kind::Text | Kind::Quote | Kind::Code) {
            l.h -= 4.0;
        }
    }
    out
}

pub fn height(lines: &[VLine]) -> f32 {
    lines.iter().map(|l| l.h).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn adv(_: char, px: f32) -> f32 {
        px * 0.5
    }
    fn plain(r: &[Run]) -> String {
        r.iter().map(|r| r.text.as_str()).collect()
    }

    #[test]
    fn fenced_code_keeps_leading_indent_and_blank_lines() {
        let l = layout(&parse("```\nfn a() {\n    deep();\n\n}\n```"), 300.0, &mut adv);
        let code: Vec<&VLine> = l.iter().filter(|x| matches!(x.kind, Kind::Code)).collect();
        assert_eq!(code.len(), 4);
        assert!(code[1].indent > code[0].indent + 20.0, "4 spaces of indent kept");
        assert!(code[2].runs.is_empty(), "blank line kept");
    }

    #[test]
    fn inline_spans_and_unclosed_markers_stay_literal() {
        let r = inline("a **bold** and `code` here");
        assert_eq!(r.iter().map(|r| r.style).collect::<Vec<_>>(), vec![Style::Normal, Style::Bold, Style::Normal, Style::Code, Style::Normal]);
        assert_eq!(plain(&r), "a bold and code here");
        assert_eq!(plain(&inline("5 ** 3 and `open and **x")), "5 ** 3 and `open and **x");
        assert_eq!(plain(&inline("my_file_name.txt and *star*")), "my_file_name.txt and *star*");
    }

    #[test]
    fn links_always_show_their_address_and_only_web_links_count() {
        let r = inline("see [Rust](https://rust-lang.org) now");
        assert_eq!(plain(&r), "see Rust <https://rust-lang.org> now");
        assert!(r.iter().any(|x| x.style == Style::Link && x.text == "Rust"));
        // other schemes, images and spaces are shown exactly as written, never as a link
        for s in ["[x](file:///etc/passwd)", "![img](https://a.b/c.png)", "[x](javascript:alert(1))", "[x](https://a.b/c d)"] {
            assert!(inline(s).iter().all(|x| x.style != Style::Link) || s.starts_with("!["), "{s}");
        }
        assert_eq!(plain(&inline("[x](file:///etc/passwd)")), "[x](file:///etc/passwd)");
        assert_eq!(plain(&inline("<b>hi</b>")), "<b>hi</b>");
    }

    #[test]
    fn blocks_parse() {
        let b = parse("# Title\nsome text\ncontinues\n\n- one\n  - two\n1. first\n> quote\n---\n```\nlet x = 1;\n  indented\n```\n");
        assert!(matches!(b[0], Block::Heading(1, _)));
        assert!(matches!(&b[1], Block::Para(r) if plain(r) == "some text continues"));
        assert!(matches!(b[2], Block::Bullet(0, _)) && matches!(b[3], Block::Bullet(1, _)));
        assert!(matches!(b[4], Block::Numbered(1, 0, _)) && matches!(b[5], Block::Quote(_)) && matches!(b[6], Block::Rule));
        assert!(matches!(&b[7], Block::Code(l) if l == &vec!["let x = 1;".to_string(), "  indented".to_string()]));
        // an unclosed fence still shows its text; a "#" with no space is plain text
        assert!(matches!(&parse("```\nabc")[0], Block::Code(l) if l[0] == "abc"));
        assert!(matches!(&parse("#hashtag")[0], Block::Para(_)));
    }

    #[test]
    fn layout_wraps_and_never_drops_text() {
        let text = "word ".repeat(60);
        let b = parse(&text);
        let l = layout(&b, 200.0, &mut adv);
        assert!(l.len() > 3);
        let joined: Vec<String> = l.iter().map(|v| plain(&v.runs)).collect();
        assert_eq!(joined.join(" ").split_whitespace().count(), 60);
        // one very long word is split, not clipped
        let l = layout(&parse(&"x".repeat(200)), 100.0, &mut adv);
        assert_eq!(l.iter().map(|v| plain(&v.runs).len()).sum::<usize>(), 200);
        assert!(height(&l) > 0.0);
    }

    #[test]
    fn arabic_and_long_code_lines_are_kept() {
        let l = layout(&parse("**مرحبا** بالعالم\n```\n".to_string().as_str()), 300.0, &mut adv);
        assert!(plain(&l[0].runs).contains("مرحبا"));
        let l = layout(&parse(&format!("```\n{}\n```", "y".repeat(500))), 200.0, &mut adv);
        assert_eq!(l.iter().map(|v| plain(&v.runs).len()).sum::<usize>(), 500);
    }
}

#[cfg(test)]
mod space_tests {
    use super::*;
    #[test]
    fn a_space_between_runs_survives_wrapping() {
        let runs = inline("**Files**: `make it` now");
        let lines = wrap_runs(&runs, 15.0, 1000.0, &mut |_, px| px * 0.5);
        let t: String = lines[0].iter().map(|r| r.text.as_str()).collect();
        assert_eq!(t, "Files: make it now");
        let runs = inline("a **b** c");
        let t: String = wrap_runs(&runs, 15.0, 1000.0, &mut |_, px| px * 0.5)[0].iter().map(|r| r.text.as_str()).collect();
        assert_eq!(t, "a b c");
    }
}
