//! Pawline native UI. Pure CPU rendering (tiny-skia + fontdue) into wl_shm
//! buffers on a Wayland layer-shell surface (xdg-toplevel fallback). No
//! OpenGL, EGL, GTK or WebKit is loaded.
mod bridge;
mod edit;
mod shape;
mod theme;
mod fsearch;
mod voice;
mod md;
mod thumb;

mod ctl;
#[cfg(unix)]
mod x11;
#[cfg(unix)]
use x11::{Xb, XEv};
#[cfg(windows)]
use winb::{Xb, XEv};
use bridge::UiReply;
#[cfg(unix)]
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer,
    data_device_manager::{
        data_device::{DataDevice, DataDeviceHandler},
        data_offer::{DataOfferHandler, DragOffer},
        data_source::{CopyPasteSource, DataSourceHandler},
        DataDeviceManagerState, WritePipe,
    },
    delegate_data_device, delegate_registry, delegate_seat, delegate_shm, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop_wayland_source::WaylandSource,
        client::{globals::registry_queue_init, protocol::*, Connection, QueueHandle},
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        xdg::{
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
            XdgShell,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use calloop::{channel, EventLoop};
#[cfg(windows)]
mod keys;
#[cfg(windows)]
mod winb;
#[cfg(windows)]
use keys::{KeyEvent, Keysym};
use std::collections::HashMap;
use std::sync::mpsc;
use tiny_skia::*;


/// Typing "/" lists Pawline's built-in skills. Choosing one only fills the input with a starter
/// phrase; nothing runs until Enter, and anything that changes things still asks first.
const SKILL_MENU: &[(&str, &str, &str)] = &[
    ("/files", "make, delete, rename, move, copy, list", "make a folder called "),
    ("/open", "open an installed app (asks first)", "open "),
    ("/wallpaper", "set a wallpaper (asks first, undoable)", "set wallpaper to "),
    ("/search", "search the web or Wikipedia", "search for "),
    ("/math", "simple math, solve for x", "what is "),
    ("/picture", "Pollinations or AI Horde (asks first)", "generate a picture of "),
    ("/ask", "online answer: general asks send, private-looking ask first", "what is "),
    ("/undo", "undo the last change", "undo"),
    ("/skills", "list skills", "/skills"),
];

/// The request that is sent: the chosen skill's starter text, then the draft. Without a skill, the draft as typed.
fn compose_request(skill: Option<usize>, draft: &str) -> String {
    match skill.and_then(|i| SKILL_MENU.get(i)) {
        Some(s) => format!("{}{}", s.2, draft.trim()),
        None => draft.trim().to_string(),
    }
}

fn menu_filter(t: &str) -> Vec<usize> {
    let t = t.to_ascii_lowercase();
    SKILL_MENU.iter().enumerate().filter(|(_, m)| m.0.starts_with(&t)).map(|(i, _)| i).collect()
}

#[cfg(test)]
mod menu_tests {
    use super::*;
    #[test]
    fn skill_chip_dispatch_is_starter_plus_draft() {
        let files = SKILL_MENU.iter().position(|m| m.0 == "/files").unwrap();
        let undo = SKILL_MENU.iter().position(|m| m.0 == "/undo").unwrap();
        let search = SKILL_MENU.iter().position(|m| m.0 == "/search").unwrap();
        assert_eq!(compose_request(Some(files), "  kitten  "), "make a folder called kitten");
        assert_eq!(compose_request(Some(undo), ""), "undo");
        assert_eq!(compose_request(Some(search), "cats"), "search for cats");
        assert_eq!(compose_request(None, " delete x "), "delete x");
        assert_eq!(compose_request(Some(999), "x"), "x", "unknown skill index is ignored");
    }

    #[test]
    fn filters_by_prefix_and_starters_never_run_themselves() {
        assert_eq!(menu_filter("/").len(), SKILL_MENU.len());
        assert_eq!(menu_filter("/pi"), vec![5]);
        assert!(menu_filter("/zzz").is_empty());
        // every starter is plain text that goes through the normal parser and its confirmations
        assert!(SKILL_MENU.iter().all(|m| !m.2.starts_with('/') || m.2 == "/skills"));
    }
}

const W: u32 = 440;
const H: u32 = 470;
const COMPACT: u32 = 68;
const BAR_W: u32 = 300;
const BAR_H: u32 = 40;
const PX: f32 = 15.0;
const LH: f32 = 20.0;
const PAD: f32 = 14.0;
const CHAT_TOP: f32 = 64.0;
const BOTTOM: f32 = 64.0;
const MAX_IN_LINES: usize = 4;
const REPLY_H: f32 = 22.0;
const THUMB_W: u32 = 220;
const THUMB_H: u32 = 150;

/// Path of a picture Pawline just saved, taken from a "Saved <path>" line. Only files directly inside a
/// Pictures/Pawline folder count, so no other text can make the app open an arbitrary file.
fn saved_picture(text: &str) -> Option<std::path::PathBuf> {
    let line = text.lines().find_map(|l| l.trim().strip_prefix("Saved "))?;
    let p = std::path::PathBuf::from(line.trim());
    let dir = p.parent()?;
    let ok = dir.file_name()? == "Pawline" && dir.parent()?.file_name()? == "Pictures" && p.is_absolute();
    ok.then_some(p)
}

/// Hard-wrap by character widths: (start, end) char ranges, never empty (an empty input is one empty line).
fn wrap_ranges(widths: &[f32], maxw: f32, spaces: &[bool]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut acc = 0.0f32;
    let mut brk: Option<usize> = None; // index just after the last space on this line
    while i < widths.len() {
        if acc + widths[i] > maxw && i > start {
            let cut = match brk {
                Some(b) if b > start && b <= i => b,
                _ => i,
            };
            out.push((start, cut));
            start = cut;
            i = cut;
            acc = 0.0;
            brk = None;
            continue;
        }
        acc += widths[i];
        if spaces[i] {
            brk = Some(i + 1);
        }
        i += 1;
    }
    out.push((start, widths.len()));
    out
}

/// Index within a line for an x offset: the nearest boundary between glyphs.
fn pick_in_line(widths: &[f32], target: f32) -> usize {
    let mut acc = 0.0;
    for (i, w) in widths.iter().enumerate() {
        if acc + w / 2.0 > target {
            return i;
        }
        acc += w;
    }
    widths.len()
}

/// Line holding the caret; a caret at a wrap boundary sits at the start of the next line.
fn line_of(lines: &[(usize, usize)], caret: usize) -> usize {
    for (k, (_, e)) in lines.iter().enumerate() {
        if caret < *e || (k + 1 == lines.len() && caret <= *e) {
            return k;
        }
    }
    lines.len() - 1
}

trait Rgba {
    fn rgba(self) -> (u8, u8, u8, u8);
}
impl Rgba for (u8, u8, u8) {
    fn rgba(self) -> (u8, u8, u8, u8) {
        (self.0, self.1, self.2, 255)
    }
}

fn settings_path() -> Option<std::path::PathBuf> {
    pawline_core::searchmem::default_path().map(|p| p.with_file_name("ui-settings.txt"))
}

/// Lines that carry a path are laid out strictly left to right with every path part in written order.
fn line_mode(l: &str) -> shape::Mode {
    if l.contains("~/") || l.starts_with('/') || l.contains(" /") || l.contains(":/") {
        shape::Mode::Strict
    } else {
        shape::Mode::Auto
    }
}

enum Req {
    Text(String, Option<std::path::PathBuf>),
    Confirm,
    Cancel,
}

#[derive(Clone, Copy, PartialEq)]
enum Btn {
    Yes,
    No,
}

struct Msg {
    user: bool,
    kind: String,
    text: String,
    /// Pawline's own plain answers are drawn as Markdown (looks only; nothing is run, opened or fetched).
    md: bool,
    thumb: Option<Pixmap>,
    /// Short preview of the message this one replied to. Local only: never sent to the model or online.
    quote: Option<String>,
}

enum VState {
    Idle,
    Listening { child: std::process::Child, path: std::path::PathBuf, since: std::time::Instant },
    Working,
}

enum VEv {
    Text(u64, Result<String, String>),
}

#[cfg(unix)]
enum Surf {
    Layer(LayerSurface),
    Win(Window),
}

#[cfg(unix)]
impl Surf {
    fn wl(&self) -> &wl_surface::WlSurface {
        match self {
            Surf::Layer(l) => l.wl_surface(),
            Surf::Win(w) => w.wl_surface(),
        }
    }
    fn commit(&self) {
        self.wl().commit();
    }
}

struct App {
    #[cfg(unix)]
    registry: Option<RegistryState>,
    #[cfg(unix)]
    seat: Option<SeatState>,
    #[cfg(unix)]
    output: Option<OutputState>,
    #[cfg(unix)]
    shm: Option<Shm>,
    #[cfg(unix)]
    pool: Option<SlotPool>,
    #[cfg(unix)]
    surf: Option<Surf>,
    #[cfg(unix)]
    qh: Option<QueueHandle<App>>,
    /// X11 backend (XFCE and other X11 sessions). None on Wayland.
    xb: Option<Xb>,
    #[cfg(unix)]
    ddm: Option<DataDeviceManagerState>,
    #[cfg(unix)]
    ddev: Option<DataDevice>,
    #[cfg(unix)]
    csrc: Option<CopyPasteSource>,
    #[cfg(unix)]
    clip_text: String,
    #[cfg(unix)]
    serial: u32,
    #[cfg(unix)]
    kb: Option<wl_keyboard::WlKeyboard>,
    #[cfg(unix)]
    ptr: Option<wl_pointer::WlPointer>,
    w: u32,
    h: u32,
    expanded: bool,
    input: edit::Edit,
    ctrl: bool,
    shift: bool,
    msgs: Vec<Msg>,
    /// Plan text awaiting a decision.
    confirm: Option<String>,
    focus: Btn,
    /// Pixels scrolled up from the bottom of the chat (0 = latest).
    scroll: f32,
    /// Pixels scrolled down from the top of the plan panel.
    plan_scroll: f32,
    busy: bool,
    menu_sel: usize,
    /// Folders from the last "zi" reply (inside the home area only); a click or Tab fills the input with "list <path>".
    zi_rows: Vec<String>,
    zi_sel: usize,
    shaper: shape::Shaper,
    /// Folder chip: full path of the folder requests mean when they name no place. Removable.
    pill: Option<std::path::PathBuf>,
    root: std::path::PathBuf,
    pill_off: bool,
    /// Rows (full paths) of the open /in folder chooser.
    pill_rows: Vec<std::path::PathBuf>,
    pill_sel: usize,
    pill_q: Option<String>,
    /// Selected skill from the "/" menu, shown as its own chip; the request is its starter text plus the draft.
    skill: Option<usize>,
    r_skill_x: (f32, f32, f32, f32),
    r_skill_name: (f32, f32, f32, f32),
    searcher: Option<fsearch::Searcher>,
    pill_gen: u64,
    stt_cmd: Option<voice::Cmd>,
    vjob: u64,
    vcancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    vnote: bool,
    vstate: VState,
    voice_tx: Option<channel::Sender<VEv>>,
    speaking: Option<std::process::Child>,
    r_mic: (f32, f32, f32, f32),
    r_vstop: (f32, f32, f32, f32),
    r_vcancel: (f32, f32, f32, f32),
    pill_wait: bool,
    /// Show the chip's full path (inspect without sending anything).
    pill_show: bool,
    r_pill_name: (f32, f32, f32, f32),
    r_pill_x: (f32, f32, f32, f32),
    settings: theme::Settings,
    settings_open: bool,
    settings_sel: usize,
    t0: std::time::Instant,
    r_gear: (f32, f32, f32, f32),
    input_w: f32,
    tx: mpsc::Sender<Req>,
    exit: bool,
    /// Panel-only: no orb. The surface is unmapped until a panel widget (or Pawline's own key) opens the card.
    panel_only: bool,
    hidden: bool,
    state: ctl::Shared,
    layer_mode: bool,
    // hit rects (x, y, w, h), set by draw
    r_yes: (f32, f32, f32, f32),
    r_no: (f32, f32, f32, f32),
    r_collapse: (f32, f32, f32, f32),
    ptr_pos: (f64, f64),
    input_top: usize,
    input_extra: f32,
    input_box_top: f32,
    input_lines: Vec<(usize, usize)>,
    dragging: bool,
    /// Closed form is a slim talk-to-it strip instead of the orb (right-click toggles). In memory only.
    bar: bool,
    /// Orb drag: (pointer x, y at the last step, moved at all?). Compact orb on a layer surface only.
    orb_drag: Option<(f64, f64, bool, (i32, i32))>,
    /// Orb position as layer margins (top, right), kept in memory only.
    margin: (i32, i32),
    bubbles: Vec<((f32, f32, f32, f32), String)>,
    toast: Option<&'static str>,
    /// Message being replied to: (full text without the Took line, short preview). Local only.
    reply: Option<(String, String)>,
    /// Max plan_scroll for the current plan panel, set by draw.
    plan_max: f32,
    chat_max: f32,
}

fn inside(r: (f32, f32, f32, f32), p: (f64, f64)) -> bool {
    let (x, y) = (p.0 as f32, p.1 as f32);
    x >= r.0 && x <= r.0 + r.2 && y >= r.1 && y <= r.1 + r.3
}

fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Path {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish().unwrap()
}

fn fill(pm: &mut Pixmap, path: &Path, c: (u8, u8, u8, u8)) {
    let mut p = Paint::default();
    p.set_color_rgba8(c.0, c.1, c.2, c.3);
    p.anti_alias = true;
    pm.fill_path(path, &p, FillRule::Winding, Transform::identity(), None);
}

fn stroke(pm: &mut Pixmap, path: &Path, c: (u8, u8, u8, u8), w: f32) {
    let mut p = Paint::default();
    p.set_color_rgba8(c.0, c.1, c.2, c.3);
    p.anti_alias = true;
    let s = Stroke { width: w, ..Default::default() };
    pm.stroke_path(path, &p, &s, Transform::identity(), None);
}

impl App {
    fn adv(&mut self, ch: char, px: f32) -> f32 {
        let mut b = [0u8; 4];
        self.shaper.measure(ch.encode_utf8(&mut b), px, shape::Mode::Auto)
    }

    fn measure(&mut self, text: &str, px: f32, mode: shape::Mode) -> f32 {
        self.shaper.measure(text, px, mode)
    }

    /// Word-wrap `text` to `maxw` pixels. Long words are split. Newlines kept.
    fn wrap(&mut self, text: &str, px: f32, maxw: f32) -> Vec<String> {
        let mut out = Vec::new();
        for para in text.split('\n') {
            let mut line = String::new();
            let mut lw = 0.0;
            for word in para.split(' ') {
                let ww: f32 = self.measure(word, px, shape::Mode::Auto);
                let sp = if line.is_empty() { 0.0 } else { self.adv(' ', px) };
                if !line.is_empty() && lw + sp + ww > maxw {
                    out.push(std::mem::take(&mut line));
                    lw = 0.0;
                }
                if ww > maxw {
                    // split an over-long word by characters
                    for c in word.chars() {
                        let cw = self.adv(c, px);
                        if lw + cw > maxw && !line.is_empty() {
                            out.push(std::mem::take(&mut line));
                            lw = 0.0;
                        }
                        line.push(c);
                        lw += cw;
                    }
                    continue;
                }
                if !line.is_empty() {
                    line.push(' ');
                    lw += self.adv(' ', px);
                }
                line.push_str(word);
                lw += ww;
            }
            out.push(line);
        }
        out
    }

    fn text(&mut self, pm: &mut Pixmap, s: &str, x: f32, base_y: f32, px: f32, c: (u8, u8, u8), clip: (f32, f32)) {
        self.text_m(pm, s, x, base_y, px, c, clip, shape::Mode::Auto);
    }

    /// A line of body text inside a box `w` wide: right-to-left lines are right-aligned, everything else left.
    /// Returns the x of the line's left edge (callers use it for carets and hit tests).
    #[allow(clippy::too_many_arguments)]
    fn para(&mut self, pm: &mut Pixmap, s: &str, x: f32, w: f32, base_y: f32, px: f32, c: (u8, u8, u8), clip: (f32, f32), mode: shape::Mode) -> f32 {
        let laid = self.shaper.layout(s, px, mode);
        let x0 = if laid.rtl { x + (w - laid.width).max(0.0) } else { x };
        self.text_m(pm, s, x0, base_y, px, c, clip, mode);
        x0
    }

    #[allow(clippy::too_many_arguments)]
    fn text_m(&mut self, pm: &mut Pixmap, s: &str, x: f32, base_y: f32, px: f32, c: (u8, u8, u8), clip: (f32, f32), mode: shape::Mode) {
        let laid = self.shaper.layout(s, px, mode);
        let (pw, ph) = (pm.width() as i32, pm.height() as i32);
        for g in &laid.glyphs {
            let cx = x + g.x;
            let (m, bm) = {
                let r = self.shaper.glyph(g.font, g.gid, px);
                (r.0, r.1.clone())
            };
            let gy = base_y as i32 - m.height as i32 - m.ymin;
            for yy in 0..m.height as i32 {
                let Y = gy + yy;
                if Y < 0 || Y >= ph || (Y as f32) < clip.0 || (Y as f32) >= clip.1 {
                    continue;
                }
                for xx in 0..m.width as i32 {
                    let a = bm[(yy as usize) * m.width + xx as usize] as u32;
                    let X = cx.round() as i32 + m.xmin + xx;
                    if a == 0 || X < 0 || X >= pw {
                        continue;
                    }
                    let i = ((Y * pw + X) * 4) as usize;
                    let d = pm.data_mut();
                    let sc = [c.0, c.1, c.2];
                    for k in 0..3 {
                        d[i + k] = ((sc[k] as u32 * a + d[i + k] as u32 * (255 - a)) / 255) as u8;
                    }
                    d[i + 3] = 255;
                }
            }
        }
    }

    fn orb(&self, pm: &mut Pixmap, cx: f32, cy: f32, r: f32, glow: bool) {
        if glow {
            let mut pb = PathBuilder::new();
            pb.push_circle(cx, cy, r + 4.0);
            fill(pm, &pb.finish().unwrap(), (224, 98, 120, 60));
        }
        let mut pb = PathBuilder::new();
        pb.push_circle(cx, cy, r);
        let g = RadialGradient::new(
            Point::from_xy(cx - r * 0.3, cy - r * 0.3),
            Point::from_xy(cx, cy),
            r * 1.2,
            vec![
                GradientStop::new(0.0, Color::from_rgba8(255, 214, 170, 255)),
                GradientStop::new(1.0, Color::from_rgba8(224, 98, 120, 255)),
            ],
            SpreadMode::Pad,
            Transform::identity(),
        )
        .unwrap();
        let mut p = Paint::default();
        p.shader = g;
        p.anti_alias = true;
        pm.fill_path(&pb.finish().unwrap(), &p, FillRule::Winding, Transform::identity(), None);
    }

    fn draw(&mut self) {
        let st = if self.hidden { "hidden" } else if self.busy { "busy" } else if self.confirm.is_some() { "confirm" } else if self.expanded { "open" } else { "idle" };
        if let Ok(mut g) = self.state.lock() {
            *g = st;
        }
        if self.hidden {
            if let Some(x) = self.xb.as_mut() {
                x.hide();
            }
            #[cfg(unix)]
            if self.xb.is_none() {
                if let Some(sf) = &self.surf {
                    sf.wl().attach(None, 0, 0);
                    sf.commit();
                }
            }
            return;
        }
        let (w, h) = (self.w, self.h);
        let mut pm = Pixmap::new(w, h).unwrap();
        if !self.expanded && self.bar {
            self.draw_bar(&mut pm);
        } else if !self.expanded {
            self.orb(&mut pm, w as f32 / 2.0, h as f32 / 2.0, 24.0, self.busy);
        } else {
            self.draw_card(&mut pm);
        }
        if self.xb.is_some() {
            let first = !self.xb.as_ref().unwrap().is_shown();
            self.x_place();
            let x = self.xb.as_mut().unwrap();
            x.show();
            x.present(&pm);
            if first || self.expanded {
                x.focus();
            }
            return;
        }
        #[cfg(unix)]
        self.present_wl(&pm);
    }

    #[cfg(unix)]
    fn present_wl(&mut self, pm: &Pixmap) {
        let (w, h) = (self.w, self.h);
        let stride = w as i32 * 4;
        let (buffer, canvas) = self
            .pool
            .as_mut()
            .unwrap()
            .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888)
            .unwrap();
        // tiny-skia: premultiplied RGBA; wl_shm Argb8888 (LE): B,G,R,A premultiplied
        for (d, s) in canvas.chunks_exact_mut(4).zip(pm.data().chunks_exact(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
        let sf = self.surf.as_ref().unwrap();
        sf.wl().damage_buffer(0, 0, w as i32, h as i32);
        buffer.attach_to(sf.wl()).unwrap();
        sf.commit();
    }

    fn th(&self) -> &'static theme::Theme {
        &theme::THEMES[self.settings.theme.min(theme::THEMES.len() - 1)]
    }

    /// Panel background: a three-colour diagonal gradient whose colours fade into each other, drifting slowly.
    fn fill_bg(&self, pm: &mut Pixmap, w: f32, h: f32) {
        let th = self.th();
        let off = theme::drift_offset(self.t0.elapsed().as_secs_f32(), w, self.settings.drift);
        let c = |k: usize| Color::from_rgba8(th.bg[k].0, th.bg[k].1, th.bg[k].2, 245);
        let g = LinearGradient::new(
            Point::from_xy(off, 0.0),
            Point::from_xy(w + off, h),
            vec![GradientStop::new(0.0, c(0)), GradientStop::new(0.5, c(1)), GradientStop::new(1.0, c(2))],
            SpreadMode::Reflect,
            Transform::identity(),
        );
        let mut p = Paint::default();
        p.anti_alias = true;
        match g {
            Some(g) => p.shader = g,
            None => p.set_color_rgba8(th.bg[0].0, th.bg[0].1, th.bg[0].2, 245),
        }
        pm.fill_path(&rrect(0.0, 0.0, w, h, 18.0), &p, FillRule::Winding, Transform::identity(), None);
    }

    fn settings_rows(&self) -> usize {
        theme::THEMES.len() + 2
    }

    fn apply_setting(&mut self, row: usize) {
        if row < theme::THEMES.len() {
            self.settings.theme = row;
        } else if row == theme::THEMES.len() {
            self.settings.drift = !self.settings.drift;
        } else {
            self.settings.speak = !self.settings.speak;
            if !self.settings.speak {
                self.stop_speaking();
            }
        }
        if let Some(p) = settings_path() {
            let _ = theme::save(&p, &self.settings);
        }
    }

    fn settings_geom(&self) -> (f32, f32) {
        (CHAT_TOP + 34.0, 34.0)
    }

    fn draw_settings(&mut self, pm: &mut Pixmap, w: f32, h: f32) {
        let _ = h;
        self.text(pm, "Settings", PAD + 4.0, CHAT_TOP + 18.0, 16.0, (235, 230, 250), (0.0, h));
        let (top, rh) = self.settings_geom();
        for k in 0..self.settings_rows() {
            let y = top + k as f32 * (rh + 6.0);
            let sel = k == self.settings_sel;
            fill(pm, &rrect(PAD - 2.0, y, w - 2.0 * PAD + 4.0, rh, 10.0), if sel { (70, 64, 110) } else { (44, 40, 64) }.rgba());
            if k < theme::THEMES.len() {
                let t = &theme::THEMES[k];
                for (i, c) in t.bg.iter().enumerate() {
                    fill(pm, &rrect(PAD + 8.0 + i as f32 * 18.0, y + 8.0, 18.0, 18.0, 4.0), (c.0, c.1, c.2, 255));
                }
                fill(pm, &rrect(PAD + 66.0, y + 8.0, 18.0, 18.0, 9.0), (t.accent.0, t.accent.1, t.accent.2, 255));
                let label = format!("Theme: {}", t.name);
                self.text(pm, &label, PAD + 96.0, y + 22.0, 14.0, (255, 255, 255), (0.0, h));
                if k == self.settings.theme {
                    self.text(pm, "selected", w - PAD - 74.0, y + 22.0, 12.0, (150, 230, 180), (0.0, h));
                }
            } else if k == theme::THEMES.len() + 1 {
                let label = format!("Speak replies aloud (local engine): {}", if self.settings.speak { "on" } else { "off" });
                self.text(pm, &label, PAD + 10.0, y + 22.0, 14.0, (255, 255, 255), (0.0, h));
            } else {
                let label = format!("Colours drift slowly: {}", if self.settings.drift { "on" } else { "off" });
                self.text(pm, &label, PAD + 10.0, y + 22.0, 14.0, (255, 255, 255), (0.0, h));
            }
        }
        let note_y = top + self.settings_rows() as f32 * (rh + 6.0) + 14.0;
        self.text(pm, "Up/Down choose, Enter applies, Esc closes.", PAD + 4.0, note_y, 12.0, (190, 185, 215), (0.0, h));
        self.text(pm, "Saved on this computer only; no request text.", PAD + 4.0, note_y + 18.0, 12.0, (190, 185, 215), (0.0, h));
    }

    fn draw_card(&mut self, pm: &mut Pixmap) {
        let (w, h) = (self.w as f32, self.h as f32);
        self.fill_bg(pm, w, h);
        self.orb(pm, 34.0, 32.0, 17.0, self.busy);
        let title = if self.busy {
            "Pawline  (working, Esc cancels)"
        } else if bridge::ONLINE_CHAT_UI.load(std::sync::atomic::Ordering::Relaxed) {
            "Pawline  [online chat ON]"
        } else {
            "Pawline"
        };
        self.text(pm, title, 62.0, 38.0, 18.0, (235, 230, 250), (0.0, h));
        // settings button (three slider lines), left of the collapse button
        self.r_gear = (w - 80.0, 14.0, 30.0, 30.0);
        fill(pm, &rrect(self.r_gear.0, self.r_gear.1, 30.0, 30.0, 8.0), if self.settings_open { self.th().accent } else { (52, 48, 70) }.rgba());
        for (k, dx) in [8.0f32, 15.0, 10.0].iter().enumerate() {
            let ly = self.r_gear.1 + 9.0 + k as f32 * 6.0;
            fill(pm, &rrect(self.r_gear.0 + 6.0, ly, 18.0, 1.6, 0.8), (220, 215, 235, 255));
            fill(pm, &rrect(self.r_gear.0 + 4.0 + dx, ly - 2.0, 4.0, 5.6, 2.0), (255, 255, 255, 255));
        }
        // collapse button
        self.r_collapse = (w - 44.0, 14.0, 30.0, 30.0);
        fill(pm, &rrect(self.r_collapse.0, self.r_collapse.1, 30.0, 30.0, 8.0), (52, 48, 70, 255));
        self.text(pm, "-", w - 33.0, 36.0, 20.0, (220, 215, 235), (0.0, h));

        let inner_w = w - 2.0 * PAD;
        if self.confirm.is_some() {
            self.draw_confirm(pm, w, h, inner_w);
        } else if self.settings_open {
            self.draw_settings(pm, w, h);
        } else {
            self.draw_chat(pm, w, h, inner_w);
            // input box: wraps onto up to MAX_IN_LINES lines, then scrolls by line
            let wmax = inner_w - 30.0;
            let chars: Vec<char> = self.input.text().chars().collect();
            self.input_w = wmax;
            let full = self.shaper.layout(&self.input.text(), PX, shape::Mode::Auto);
            let widths: Vec<f32> = full.ext.iter().map(|e| (e.1 - e.0).max(0.0)).collect();
            let spaces: Vec<bool> = chars.iter().map(|c| *c == ' ').collect();
            let lines = wrap_ranges(&widths, wmax, &spaces);
            let nl = lines.len().clamp(1, MAX_IN_LINES);
            let caret = self.input.caret();
            let cl = line_of(&lines, caret);
            if cl < self.input_top {
                self.input_top = cl;
            }
            if cl >= self.input_top + nl {
                self.input_top = cl + 1 - nl;
            }
            self.input_top = self.input_top.min(lines.len().saturating_sub(nl));
            self.input_extra = (nl as f32 - 1.0) * LH + self.chip_h();
            let bh = 40.0 + self.input_extra;
            let top = h - 12.0 - bh;
            self.input_box_top = top;
            if let Some((_, prev)) = &self.reply {
                let sy = top - REPLY_H;
                fill(pm, &rrect(PAD - 2.0, sy, w - 2.0 * PAD + 4.0, REPLY_H + 6.0, 8.0), (40, 36, 60, 255));
                fill(pm, &rrect(PAD + 4.0, sy + 4.0, 3.0, 14.0, 1.5), (34, 211, 238, 255));
                let label = format!("Reply (local, not sent): {prev}   [x]");
                self.text(pm, &label, PAD + 12.0, sy + 16.0, 12.0, (200, 195, 225), (0.0, h));
            }
            fill(pm, &rrect(PAD - 2.0, top, w - 2.0 * PAD + 4.0, bh, 12.0), self.th().input.rgba());
            let col = if self.busy { (150, 145, 165) } else { (255, 255, 255) };
            let sel = self.input.sel();
            for k in 0..nl {
                let li = self.input_top + k;
                let (ls, le) = lines[li];
                let ly = top + 6.0 + self.chip_h() + k as f32 * LH;
                let text: String = chars[ls..le].iter().collect();
                let laid = self.shaper.layout(&text, PX, shape::Mode::Auto);
                let x0 = PAD + 8.0 + if laid.rtl { (wmax - laid.width).max(0.0) } else { 0.0 };
                if let Some((sa, sb)) = sel {
                    let (a, b) = (sa.max(ls), sb.min(le));
                    if b > a {
                        for (l, r) in laid.sel_rects(a - ls, b - ls) {
                            fill(pm, &rrect(x0 + l, ly, r - l, LH, 3.0), self.th().accent.rgba());
                        }
                    }
                }
                self.text(pm, &text, x0, ly + 15.0, PX, col, (0.0, h));
                if li == cl {
                    let cx = laid.caret_x(caret.min(le) - ls);
                    fill(pm, &rrect(x0 + cx, ly + 1.0, 2.0, LH - 2.0, 1.0), (255, 255, 255, 255));
                }
            }
            self.input_lines = lines;
            if let Some(t) = self.toast {
                let tw = self.measure(t, 12.0, shape::Mode::Auto);
                self.text(pm, t, (w - PAD - 36.0 - tw).max(PAD), top - 6.0, 12.0, (170, 165, 195), (0.0, h));
            }
            self.draw_menu(pm, w, h);
            self.pill_refresh();
            self.draw_zi(pm, w, h);
            self.draw_pill_rows(pm, w, h);
            self.draw_pill(pm, h);
            self.draw_voice(pm, w, h);
        }
    }

    fn draw_chat(&mut self, pm: &mut Pixmap, w: f32, h: f32, inner_w: f32) {
        // Scroll hints get their own reserved rows (only while scrolled), so they never cover text.
        let hint_top = self.chat_max > 0.0 && self.scroll < self.chat_max - 1.0;
        let hint_bot = self.scroll > 1.0;
        let top0 = CHAT_TOP;
        let bot0 = h - BOTTOM - self.input_extra + 4.0 - if self.reply.is_some() { REPLY_H } else { 0.0 };
        let top = top0 + if hint_top { 20.0 } else { 0.0 };
        let bot = bot0 - if hint_bot { 20.0 } else { 0.0 };
        // layout all messages
        let bub_w = inner_w - 40.0;
        let mut items: Vec<(bool, String, Vec<String>, f32, Option<Pixmap>, Option<String>, Option<Vec<md::VLine>>)> = Vec::new();
        let mut texts: Vec<String> = Vec::new();
        self.bubbles.clear();
        let mut total = 0.0;
        let msgs: Vec<(bool, String, String, Option<Pixmap>, Option<String>, bool)> = self.msgs.iter().map(|m| (m.user, m.kind.clone(), m.text.clone(), m.thumb.clone(), m.quote.clone(), m.md)).collect();
        for (user, kind, text, thumb, quote, is_md) in msgs {
            let mdl = if is_md {
                let spaced = match text.rsplit_once("\nTook ") {
                    Some((b, t)) => format!("{b}\n\nTook {t}"),
                    None => text.clone(),
                };
                let blocks = md::parse(&spaced);
                let sh = &mut self.shaper;
                Some(md::layout(&blocks, bub_w - 20.0, &mut |c, px| {
                    let mut b = [0u8; 4];
                    sh.measure(c.encode_utf8(&mut b), px, shape::Mode::Auto)
                }))
            } else {
                None
            };
            let lines = if mdl.is_some() { vec![] } else { self.wrap(&text, PX, bub_w - 20.0) };
            let body_h = match &mdl {
                Some(m) => md::height(m),
                None => lines.len() as f32 * LH,
            };
            let bh = body_h + 14.0 + quote.as_ref().map_or(0.0, |_| 18.0) + thumb.as_ref().map_or(0.0, |t| t.height() as f32 + 6.0);
            total += bh + 8.0;
            texts.push(text.clone());
            items.push((user, kind, lines, bh, thumb, quote, mdl));
        }
        let view = bot - top;
        self.chat_max = (total - view).max(0.0);
        if self.scroll > self.chat_max {
            self.scroll = self.chat_max;
        }
        // y of content start so that the bottom aligns with `bot` + scroll
        let mut cp = Pixmap::new(w as u32, view as u32).unwrap();
        let mut y = view - total + self.scroll;
        for (idx, (user, kind, lines, bh, thumb, quote, mdl)) in items.into_iter().enumerate() {
            let mut bw0 = 0.0f32;
            for l in &lines {
                bw0 = bw0.max(self.measure(l, PX, line_mode(l)));
            }
            let qw = match &quote {
                Some(q) => (self.measure(q, 12.0, shape::Mode::Auto) + 8.0).min(bub_w - 20.0),
                None => 0.0,
            };
            if let Some(m) = &mdl {
                for vl in m {
                    let t: String = vl.runs.iter().map(|r| r.text.as_str()).collect();
                    bw0 = bw0.max(vl.indent + self.measure(&t, vl.px, line_mode(&t)) + if vl.marker.is_some() { 0.0 } else { 0.0 });
                }
            }
            let bw = bw0.max(qw).max(thumb.as_ref().map_or(0.0, |t| t.width() as f32)) + 20.0;
            let bx = if user { w - PAD - bw } else { PAD };
            if y + bh > 0.0 && y < view {
                self.bubbles.push(((bx, top + y.max(0.0), bw, (bh).min(view - y.max(0.0)).min(y + bh)), texts[idx].clone()));
                let col = if user {
                    self.th().user.rgba()
                } else if kind == "error" || kind == "failed" {
                    (92, 40, 52, 255)
                } else {
                    (46, 42, 64, 255)
                };
                fill(&mut cp, &rrect(bx, y, bw, bh, 12.0), col);
                let qh = quote.as_ref().map_or(0.0, |_| 18.0);
                if let Some(q) = &quote {
                    fill(&mut cp, &rrect(bx + 8.0, y + 7.0, 3.0, 14.0, 1.5), (34, 211, 238, 255));
                    self.text(&mut cp, q, bx + 16.0, y + 18.0, 12.0, (170, 165, 195), (0.0, view));
                }
                if let Some(m) = &mdl {
                    let mut yy = y + qh + 7.0;
                    for vl in m {
                        self.draw_vline(&mut cp, vl, bx + 10.0, bw - 20.0, yy, (0.0, view));
                        yy += vl.h;
                    }
                }
                for (i, l) in lines.iter().enumerate() {
                    self.para(&mut cp, l, bx + 10.0, bw - 20.0, y + qh + 7.0 + (i as f32 + 1.0) * LH - 5.0, PX, (235, 232, 245), (0.0, view), line_mode(l));
                }
                if let Some(t) = &thumb {
                    let body_h = mdl.as_ref().map_or(lines.len() as f32 * LH, |m| md::height(m));
                    let ty = y + qh + 7.0 + body_h + 2.0;
                    cp.draw_pixmap((bx + 10.0) as i32, ty as i32, t.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
                }
            }
            y += bh + 8.0;
        }
        pm.draw_pixmap(0, top as i32, cp.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        if hint_top {
            self.text(pm, "^ older (PageUp / wheel)", PAD + 6.0, top0 + 14.0, 12.0, (170, 165, 195), (0.0, h));
        }
        if hint_bot {
            self.text(pm, "v newer (PageDown)", PAD + 6.0, bot0 - 5.0, 12.0, (170, 165, 195), (0.0, h));
        }
    }

    /// One Markdown line: heading size, bold (drawn twice, 0.7 px apart), code on a darker pad, links underlined
    /// with their address visible, quote bar, bullet markers. Text only; nothing here is clickable.
    fn draw_vline(&mut self, pm: &mut Pixmap, vl: &md::VLine, x: f32, w: f32, top: f32, clip: (f32, f32)) {
        let base = top + vl.h - 5.0 - if matches!(vl.kind, md::Kind::Heading) { 3.0 } else { 0.0 };
        match vl.kind {
            md::Kind::Rule => {
                fill(pm, &rrect(x, top + vl.h / 2.0, w, 1.5, 0.7), (110, 104, 140, 255));
                return;
            }
            md::Kind::Quote => fill(pm, &rrect(x, top, 3.0, vl.h, 1.5), (34, 211, 238, 255)),
            md::Kind::Code => {
                let tw: f32 = vl.runs.iter().map(|r| r.text.as_str()).collect::<String>().chars().count() as f32;
                let _ = tw;
                fill(pm, &rrect(x, top, w, vl.h, 4.0), (28, 25, 42, 255));
            }
            _ => {}
        }
        if let Some(m) = &vl.marker {
            self.text(pm, m, x + vl.indent - 16.0, base, vl.px, (190, 185, 215), clip);
        }
        let all: String = vl.runs.iter().map(|r| r.text.as_str()).collect();
        if shape::has_rtl(&all) {
            // right-to-left lines are drawn as one piece (bold/code styling is dropped on those lines)
            self.para(pm, &all, x + vl.indent, w - vl.indent, base, vl.px, (235, 232, 245), clip, line_mode(&all));
            return;
        }
        let mut cx = x + vl.indent + if matches!(vl.kind, md::Kind::Code) { 0.0 } else { 0.0 };
        for r in &vl.runs {
            let wdt = self.measure(&r.text, vl.px, line_mode(&r.text));
            let (col, boldish) = match (r.style, &vl.kind) {
                (md::Style::Bold, _) | (_, md::Kind::Heading) => ((255, 255, 255), true),
                (md::Style::Code, _) | (_, md::Kind::Code) => ((150, 230, 220), false),
                (md::Style::Link, _) => ((120, 190, 255), false),
                (_, md::Kind::Quote) => ((190, 185, 215), false),
                _ => ((235, 232, 245), false),
            };
            if matches!(r.style, md::Style::Code) && !matches!(vl.kind, md::Kind::Code) {
                fill(pm, &rrect(cx - 2.0, top + 1.0, wdt + 4.0, vl.h - 2.0, 4.0), (28, 25, 42, 255));
            }
            self.text_m(pm, &r.text, cx, base, vl.px, col, clip, line_mode(&r.text));
            if boldish {
                self.text_m(pm, &r.text, cx + 0.7, base, vl.px, col, clip, line_mode(&r.text));
            }
            if matches!(r.style, md::Style::Link) {
                fill(pm, &rrect(cx, base + 2.0, wdt, 1.2, 0.5), (120, 190, 255, 255));
            }
            cx += wdt;
        }
    }

    fn draw_confirm(&mut self, pm: &mut Pixmap, w: f32, h: f32, inner_w: f32) {
        let text = self.confirm.clone().unwrap();
        let top = CHAT_TOP;
        let bot = h - BOTTOM - 8.0;
        let panel = rrect(PAD - 4.0, top, w - 2.0 * PAD + 8.0, bot - top, 14.0);
        fill(pm, &panel, (29, 20, 64, 255));
        stroke(pm, &panel, (124, 58, 237, 255), 1.5);
        let text = if shape::has_bidi_control(&text) {
            format!("WARNING: a name below contains hidden text-direction characters (shown as a replacement mark). Check the full path carefully.\n{text}")
        } else {
            text
        };
        let lines = self.wrap(&text, PX, inner_w - 16.0);
        let total = lines.len() as f32 * LH + 16.0;
        let view = bot - top - 6.0;
        self.plan_max = (total - view).max(0.0);
        if self.plan_scroll > self.plan_max {
            self.plan_scroll = self.plan_max;
        }
        for (i, l) in lines.iter().enumerate() {
            let y = top + 8.0 + (i as f32 + 1.0) * LH - 5.0 - self.plan_scroll;
            self.text_m(pm, l, PAD + 4.0, y, PX, (235, 230, 255), (top + 3.0, bot - 3.0), shape::Mode::Strict);
        }
        let can_yes = self.can_approve();
        if self.plan_max > 0.0 && !can_yes {
            self.text(pm, "scroll down to read the whole plan (Down / PageDown / wheel)", PAD + 2.0, bot + 12.0, 12.0, (250, 200, 120), (0.0, h));
        }
        // buttons
        let by = h - 46.0;
        self.r_yes = (PAD, by, 120.0, 34.0);
        self.r_no = (PAD + 136.0, by, 120.0, 34.0);
        let (yes_bg, yes_fg) = if can_yes { ((46, 125, 80, 255), (255, 255, 255)) } else { ((60, 60, 66, 255), (130, 130, 140)) };
        fill(pm, &rrect(self.r_yes.0, by, 120.0, 34.0, 10.0), yes_bg);
        fill(pm, &rrect(self.r_no.0, by, 120.0, 34.0, 10.0), (110, 60, 70, 255));
        self.text(pm, "Do it", self.r_yes.0 + 36.0, by + 23.0, PX, yes_fg, (0.0, h));
        self.text(pm, "Cancel", self.r_no.0 + 30.0, by + 23.0, PX, (255, 255, 255), (0.0, h));
        let fr = if self.focus == Btn::Yes { self.r_yes } else { self.r_no };
        stroke(pm, &rrect(fr.0 - 2.0, fr.1 - 2.0, fr.2 + 4.0, fr.3 + 4.0, 12.0), (34, 211, 238, 255), 2.5);
        self.text(pm, "Left/Right, Enter", PAD + 280.0, by + 21.0, 12.0, (150, 145, 175), (0.0, h));
    }

    /// "Do it" is enabled only when the entire plan has been seen.
    fn can_approve(&self) -> bool {
        self.confirm.is_some() && !self.busy && self.plan_scroll >= self.plan_max - 1.0
    }

    fn on_ctl(&mut self, c: ctl::Ctl) {
        match c {
            ctl::Ctl::Show => self.set_expanded(true),
            ctl::Ctl::Hide => {
                self.hidden = true;
                self.expanded = false;
                self.draw();
            }
            ctl::Ctl::Toggle => {
                if self.hidden || !self.expanded {
                    self.set_expanded(true);
                } else {
                    self.set_expanded(false);
                }
            }
        }
    }

    fn set_expanded(&mut self, exp: bool) {
        self.expanded = exp;
        self.hidden = !exp && self.panel_only;
        if exp {
            self.w = W;
            self.h = H;
        } else {
            (self.w, self.h) = if self.bar { (BAR_W, BAR_H) } else { (COMPACT, COMPACT) };
        }
        #[cfg(unix)]
        match self.surf.as_ref() {
            None => {}
            Some(Surf::Layer(l)) => {
                // The open card docks in the corner so it always fits the screen; the orb returns to where it was left.
                let m = if exp { (12, 12) } else { self.margin };
                l.set_margin(m.0, m.1, 0, 0);
                l.set_size(self.w, self.h)
            }
            Some(Surf::Win(win)) => {
                win.set_min_size(Some((self.w, self.h)));
                win.set_max_size(Some((self.w, self.h)));
            }
        }
        self.draw();
    }

    /// Closed form: orb <-> slim strip you can type into.
    fn toggle_bar(&mut self) {
        if self.expanded {
            return;
        }
        self.bar = !self.bar;
        self.set_expanded(false);
    }

    fn draw_bar(&mut self, pm: &mut Pixmap) {
        let (w, h) = (self.w as f32, self.h as f32);
        fill(pm, &rrect(0.0, 0.0, w, h, 14.0), (30, 28, 40, 245));
        self.orb(pm, 20.0, h / 2.0, 11.0, self.busy);
        let chars: Vec<char> = self.input.text().chars().collect();
        if chars.is_empty() {
            self.text(pm, "Ask Pawline...", 40.0, h / 2.0 + 5.0, PX, (150, 145, 175), (0.0, w));
            return;
        }
        let full = self.shaper.layout(&self.input.text(), PX, shape::Mode::Auto);
        let widths: Vec<f32> = full.ext.iter().map(|e| (e.1 - e.0).max(0.0)).collect();
        let avail = w - 40.0 - 12.0;
        let caret = self.input.caret().min(chars.len());
        let mut start = 0;
        while start < caret && widths[start..caret].iter().sum::<f32>() > avail - 4.0 {
            start += 1;
        }
        let mut end = start;
        let mut used = 0.0;
        while end < chars.len() && used + widths[end] <= avail {
            used += widths[end];
            end += 1;
        }
        let shown: String = chars[start..end].iter().collect();
        let laid = self.shaper.layout(&shown, PX, shape::Mode::Auto);
        let x0 = 40.0 + if laid.rtl { (avail - laid.width).max(0.0) } else { 0.0 };
        self.text(pm, &shown, x0, h / 2.0 + 5.0, PX, (255, 255, 255), (0.0, w));
        let cx = laid.caret_x(caret.min(end) - start);
        fill(pm, &rrect(x0 + cx, h / 2.0 - 9.0, 2.0, 18.0, 1.0), (255, 255, 255, 255));
    }

    fn push(&mut self, user: bool, kind: &str, text: String, quote: Option<String>) {
        let thumb = if user { None } else { saved_picture(&text).and_then(|p| thumb::load(&p, THUMB_W, THUMB_H)) };
        self.msgs.push(Msg { user, kind: kind.into(), text, md: false, thumb, quote });
        self.scroll = 0.0;
    }

    /// Skill-menu rows matching what is typed (only a bare "/word", nothing else on screen).
    fn menu_rows(&self) -> Vec<usize> {
        let t = self.input.text();
        if self.busy || self.confirm.is_some() || !t.starts_with('/') || t.contains(' ') {
            return vec![];
        }
        menu_filter(&t)
    }

    /// Fill the input from the highlighted menu row. Returns true if the key was used.
    fn menu_pick(&mut self) -> bool {
        let rows = self.menu_rows();
        if rows.is_empty() {
            return false;
        }
        let i = rows[self.menu_sel.min(rows.len() - 1)];
        // the skill becomes its own chip (a second pick replaces it); the draft stays in the box
        self.input.clear();
        self.skill = Some(i);
        self.menu_sel = 0;
        true
    }

    fn vcmds(&self) -> Result<(voice::Cmd, voice::Cmd, std::path::PathBuf), String> {
        let wav = voice::temp_wav();
        let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let has = &voice::has_in_path;
        let rec = voice::recorder(std::env::var("PAWLINE_REC_CMD").ok().as_deref(), has, &wav)?;
        let setup = voice::find_stt(&home, &|b| voice::has_in_path(b));
        let st = voice::stt(std::env::var("PAWLINE_STT_CMD").ok().as_deref(), has, &setup, &wav)?;
        Ok((rec, st, wav))
    }

    /// Microphone button: start listening. Everything needed is checked first; nothing records if something is missing.
    fn start_listening(&mut self) {
        if !matches!(self.vstate, VState::Idle) || self.busy {
            return;
        }
        match self.vcmds().and_then(|(rec, st, wav)| voice::start_recording(&rec, &wav).map(|c| (c, st, wav))) {
            Ok((child, st, wav)) => {
                self.stt_cmd = Some(st);
                self.vstate = VState::Listening { child, path: wav, since: std::time::Instant::now() };
            }
            Err(e) => self.push(false, "error", format!("Voice input is not available: {e}"), None),
        }
        self.draw();
    }

    /// Stop recording and turn the audio into text on this computer (off the UI thread). The text goes into the draft.
    fn stop_listening(&mut self) {
        let VState::Listening { child, path, .. } = std::mem::replace(&mut self.vstate, VState::Working) else {
            return;
        };
        let (Some(st), Some(tx)) = (self.stt_cmd.clone(), self.voice_tx.clone()) else {
            voice::cancel_recording(child, &path);
            self.vstate = VState::Idle;
            return;
        };
        self.vjob += 1;
        let job = self.vjob;
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.vcancel = cancel.clone();
        std::thread::spawn(move || {
            voice::stop_recording(child);
            let r = voice::transcribe(&st, &path, voice::STT_TIMEOUT, &cancel);
            let _ = tx.send(VEv::Text(job, r));
        });
        self.draw();
    }

    /// Cancel: the recorder is killed and the audio file deleted; nothing is transcribed or added.
    fn cancel_listening(&mut self) {
        if let VState::Listening { child, path, .. } = std::mem::replace(&mut self.vstate, VState::Idle) {
            voice::cancel_recording(child, &path);
            self.toast = Some("Recording discarded");
        }
        self.draw();
    }

    /// Cancel while the text is being made: the engine is killed and whatever it returns is dropped.
    fn cancel_working(&mut self) {
        if matches!(self.vstate, VState::Working) {
            self.vcancel.store(true, std::sync::atomic::Ordering::SeqCst);
            self.vstate = VState::Idle;
            self.toast = Some("Cancelled. Your draft is unchanged");
        }
        self.draw();
    }

    fn on_voice(&mut self, ev: VEv) {
        let VEv::Text(job, _) = &ev;
        if *job != self.vjob || !matches!(self.vstate, VState::Working) {
            return; // cancelled or stale: drop it
        }
        self.vstate = VState::Idle;
        match ev {
            VEv::Text(_, Ok(t)) => {
                let cur = self.input.text();
                if !cur.is_empty() && !cur.ends_with(' ') {
                    self.input.insert(" ");
                }
                self.input.insert(&t);
                self.vnote = true;
            }
            VEv::Text(_, Err(e)) => self.push(false, "error", e, None),
        }
        self.draw();
    }

    fn stop_speaking(&mut self) {
        if let Some(mut c) = self.speaking.take() {
            voice::kill_group(&mut c);
        }
    }

    fn speak_reply(&mut self, text: &str) {
        self.stop_speaking();
        let t = voice::clean_for_speech(text);
        if t.is_empty() {
            return;
        }
        match voice::tts(std::env::var("PAWLINE_TTS_CMD").ok().as_deref(), &voice::has_in_path).and_then(|c| voice::speak(&c, &t)) {
            Ok(ch) => self.speaking = Some(ch),
            Err(e) => self.push(false, "error", format!("Spoken replies are on but: {e}"), None),
        }
    }

    /// Once a second: update the timer, stop a recording at the time limit, notice that speech finished.
    fn voice_tick(&mut self) {
        let mut redraw = false;
        if let VState::Listening { since, .. } = &self.vstate {
            redraw = true;
            if since.elapsed() >= voice::MAX_RECORD {
                self.stop_listening();
            }
        }
        if let Some(c) = self.speaking.as_mut() {
            if matches!(c.try_wait(), Ok(Some(_)) | Err(_)) {
                self.speaking = None;
            }
            redraw = true;
        }
        if redraw {
            self.draw();
        }
    }

    fn draw_voice(&mut self, pm: &mut Pixmap, w: f32, h: f32) {
        let mx = w - PAD - 26.0;
        let my = self.input_box_top + 9.0;
        self.r_mic = (mx, my, 22.0, 22.0);
        let listening = matches!(self.vstate, VState::Listening { .. });
        fill(pm, &rrect(mx, my, 22.0, 22.0, 11.0), if listening { (200, 50, 60) } else { self.th().accent }.rgba());
        fill(pm, &rrect(mx + 8.0, my + 4.0, 6.0, 9.0, 3.0), (255, 255, 255, 255));
        fill(pm, &rrect(mx + 10.5, my + 14.0, 1.2, 4.0, 0.5), (255, 255, 255, 255));
        fill(pm, &rrect(mx + 7.0, my + 17.5, 8.0, 1.2, 0.5), (255, 255, 255, 255));
        self.r_vstop = (0.0, 0.0, 0.0, 0.0);
        self.r_vcancel = (0.0, 0.0, 0.0, 0.0);
        let bar_y = self.input_box_top - 34.0;
        let bw = w - 2.0 * PAD + 4.0;
        let label = match &self.vstate {
            VState::Listening { since, .. } => Some(format!("Listening {}:{:02}  (on this computer, not uploaded)", since.elapsed().as_secs() / 60, since.elapsed().as_secs() % 60)),
            VState::Working => Some("Turning speech into text on this computer...".to_string()),
            VState::Idle if self.speaking.is_some() => Some("Speaking...".to_string()),
            VState::Idle => None,
        };
        let Some(label) = label else {
            if self.vnote {
                // Safety text: the transcript can be wrong. Own rows, wrapped, never clipped; stays until a key press.
                let msg = "Check this text before Enter: speech-to-text can be wrong.";
                fill(pm, &rrect(PAD - 2.0, bar_y, bw, 28.0, 10.0), (70, 52, 14, 255));
                self.text(pm, msg, PAD + 8.0, bar_y + 19.0, 12.0, (255, 235, 180), (0.0, h));
            }
            return;
        };
        fill(pm, &rrect(PAD - 2.0, bar_y, bw, 28.0, 10.0), (60, 24, 32, 255));
        fill(pm, &rrect(PAD + 6.0, bar_y + 10.0, 8.0, 8.0, 4.0), (240, 70, 80, 255));
        self.text(pm, &label, PAD + 20.0, bar_y + 19.0, 12.0, (255, 255, 255), (0.0, h));
        let (sx, cx) = (w - PAD - 130.0, w - PAD - 64.0);
        if listening {
            fill(pm, &rrect(sx, bar_y + 4.0, 60.0, 20.0, 8.0), (46, 125, 80, 255));
            self.text(pm, "Stop", sx + 15.0, bar_y + 18.0, 12.0, (255, 255, 255), (0.0, h));
            self.r_vstop = (sx, bar_y + 4.0, 60.0, 20.0);
        }
        let working = matches!(self.vstate, VState::Working);
        if listening || working || self.speaking.is_some() {
            fill(pm, &rrect(cx, bar_y + 4.0, 60.0, 20.0, 8.0), (110, 50, 60, 255));
            self.text(pm, if listening || working { "Cancel" } else { "Stop" }, cx + 12.0, bar_y + 18.0, 12.0, (255, 255, 255), (0.0, h));
            self.r_vcancel = (cx, bar_y + 4.0, 60.0, 20.0);
        }
    }

    fn chip_h(&self) -> f32 {
        if self.pill.is_some() || self.skill.is_some() {
            28.0
        } else {
            0.0
        }
    }

    /// "/in" as its own word: (draft before it, search words after it).
    fn in_query(&self) -> Option<(String, String)> {
        let t = self.input.text();
        let (idx, _) = t.rmatch_indices("/in").find(|(i, _)| {
            let before_ok = *i == 0 || t[..*i].ends_with(' ');
            let after = &t[*i + 3..];
            before_ok && (after.is_empty() || after.starts_with(' '))
        })?;
        Some((t[..idx].to_string(), t[idx + 3..].trim_start().to_string()))
    }

    fn pill_active(&self) -> bool {
        self.in_query().is_some() && !self.pill_off && !self.busy && self.confirm.is_none() && self.expanded
    }

    /// Typing "/in" (and words after it) opens and filters the chooser; called after every edit.
    fn pill_refresh(&mut self) {
        match self.in_query() {
            None => {
                self.pill_off = false;
                self.pill_rows.clear();
                self.pill_q = None;
                if self.pill_wait {
                    self.pill_wait = false;
                    if let Some(s) = &self.searcher {
                        s.cancel();
                    }
                }
            }
            Some((_, q)) => {
                if self.pill_active() && self.pill_q.as_deref() != Some(q.as_str()) {
                    self.pill_q = Some(q.clone());
                    self.start_search(q);
                }
            }
        }
    }

    /// Search off the UI thread: a new generation cancels the old walk; the worker waits 120 ms first (debounce)
    /// and sends nothing back if it was superseded. Results come back through the event loop.
    fn start_search(&mut self, q: String) {
        self.pill_rows.clear();
        self.pill_sel = 0;
        match &self.searcher {
            Some(s) => {
                self.pill_wait = true;
                self.pill_gen = s.request(&q);
            }
            None => self.pill_rows = bridge::folder_candidates(&self.root, &q, &|| false),
        }
    }

    fn on_search(&mut self, gen: u64, q: String, rows: Vec<std::path::PathBuf>) {
        if !fsearch::accept_result(self.pill_gen, self.pill_q.as_deref(), gen, &q) {
            return; // stale result
        }
        self.pill_wait = false;
        self.pill_rows = rows;
        self.pill_sel = 0;
        self.draw();
    }

    /// Pick the highlighted folder: the "/in ..." words leave the draft, the rest of the draft stays, a chip appears.
    fn pill_pick(&mut self) {
        if !fsearch::can_pick(self.pill_wait, self.pill_rows.len()) {
            return;
        }
        if let (Some(p), Some((before, _))) = (self.pill_rows.get(self.pill_sel.min(self.pill_rows.len().saturating_sub(1))).cloned(), self.in_query()) {
            self.input.clear();
            self.input.insert(&before);
            self.pill = Some(p);
            self.pill_show = false;
        }
        self.pill_rows.clear();
        self.pill_q = None;
    }

    const PILL_VIS: usize = 8;

    fn pill_window(&self) -> usize {
        let n = self.pill_rows.len();
        let sel = self.pill_sel.min(n.saturating_sub(1));
        sel.saturating_sub(Self::PILL_VIS - 1).min(n.saturating_sub(Self::PILL_VIS))
    }

    fn pill_geom(&self) -> (f32, f32) {
        let rows_h = self.pill_rows.len().clamp(1, Self::PILL_VIS) as f32 * 26.0;
        (self.input_box_top - 6.0 - rows_h - 30.0, rows_h)
    }

    fn draw_pill_rows(&mut self, pm: &mut Pixmap, w: f32, h: f32) {
        if !self.pill_active() {
            return;
        }
        let (top, rows_h) = self.pill_geom();
        fill(pm, &rrect(PAD - 2.0, top, w - 2.0 * PAD + 4.0, rows_h + 26.0, 10.0), (36, 33, 58, 255));
        if self.pill_rows.is_empty() && self.pill_wait {
            self.text(pm, "Searching...", PAD + 10.0, top + 20.0, 13.0, (200, 200, 230), (0.0, h));
        } else if self.pill_rows.is_empty() {
            self.text(pm, "No folder matches. Keep typing, or Esc.", PAD + 10.0, top + 20.0, 13.0, (230, 200, 200), (0.0, h));
        }
        let n = self.pill_rows.len();
        let start = self.pill_window();
        let sel = self.pill_sel.min(n.saturating_sub(1));
        let rows: Vec<std::path::PathBuf> = self.pill_rows.iter().skip(start).take(Self::PILL_VIS).cloned().collect();
        for (k, p) in rows.iter().enumerate() {
            let y = top + 4.0 + k as f32 * 26.0;
            if start + k == sel {
                fill(pm, &rrect(PAD + 2.0, y, w - 2.0 * PAD - 4.0, 24.0, 6.0), self.th().accent.rgba());
            }
            let full = p.to_string_lossy().to_string();
            let shown = if full.chars().count() > 44 { format!("...{}", full.chars().rev().take(41).collect::<Vec<_>>().into_iter().rev().collect::<String>()) } else { full };
            self.text_m(pm, &shown, PAD + 10.0, y + 18.0, 13.0, (255, 255, 255), (0.0, h), shape::Mode::Strict);
        }
        let hint = format!("{} of {}  Up/Down, Enter or click picks, type to search, Esc", if n == 0 { 0 } else { sel + 1 }, n);
        self.text(pm, &hint, PAD + 10.0, top + 4.0 + rows_h + 15.0, 11.0, (190, 185, 215), (0.0, h));
    }

    /// The chip lives inside the composer (top row of the input box): "/in" skill label, last folder name, x to remove.
    /// A click on the name shows or hides the full path above the box. Nothing is sent.
    fn draw_pill(&mut self, pm: &mut Pixmap, h: f32) {
        self.r_pill_x = (0.0, 0.0, 0.0, 0.0);
        self.r_pill_name = (0.0, 0.0, 0.0, 0.0);
        self.r_skill_x = (0.0, 0.0, 0.0, 0.0);
        self.r_skill_name = (0.0, 0.0, 0.0, 0.0);
        let (mut x, y, ph) = (PAD + 4.0, self.input_box_top + 5.0, 22.0);
        if let Some(s) = self.skill.and_then(|i| SKILL_MENU.get(i)) {
            let tw = self.measure(s.0, 13.0, shape::Mode::Strict);
            let pw = tw + 12.0 + 26.0;
            fill(pm, &rrect(x, y, pw, ph, 11.0), (34, 120, 140, 255));
            self.text(pm, s.0, x + 8.0, y + 16.0, 13.0, (255, 255, 255), (0.0, h));
            self.text(pm, "x", x + pw - 18.0, y + 16.0, 13.0, (235, 245, 250), (0.0, h));
            self.r_skill_x = (x + pw - 26.0, y, 26.0, ph);
            self.r_skill_name = (x, y, pw - 26.0, ph);
            x += pw + 6.0;
        }
        let Some(p) = self.pill.clone() else {
            return;
        };
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "/".into());
        let shown = if name.chars().count() > 24 { format!("{}...", name.chars().take(21).collect::<String>()) } else { name };
        let label = "/in";
        let lw = self.measure(label, 12.0, shape::Mode::Strict);
        let tw = self.measure(&shown, 13.0, shape::Mode::Strict);
        let pw = lw + tw + 12.0 + 8.0 + 26.0;
        fill(pm, &rrect(x, y, pw, ph, 11.0), self.th().accent.rgba());
        self.text(pm, label, x + 8.0, y + 16.0, 12.0, (220, 215, 245), (0.0, h));
        self.text_m(pm, &shown, x + 8.0 + lw + 6.0, y + 16.0, 13.0, (255, 255, 255), (0.0, h), shape::Mode::Strict);
        self.text(pm, "x", x + pw - 18.0, y + 16.0, 13.0, (235, 230, 250), (0.0, h));
        self.r_pill_x = (x + pw - 26.0, y, 26.0, ph);
        self.r_pill_name = (x, y, pw - 26.0, ph);
        if self.pill_show {
            let full = p.to_string_lossy().to_string();
            let ty = self.input_box_top - 28.0;
            let wd = self.w as f32 - 2.0 * PAD + 4.0;
            fill(pm, &rrect(PAD - 2.0, ty, wd, 22.0, 8.0), (20, 18, 32, 255));
            let shown = if self.measure(&full, 12.0, shape::Mode::Strict) > wd - 16.0 {
                format!("...{}", full.chars().rev().take(46).collect::<Vec<_>>().into_iter().rev().collect::<String>())
            } else {
                full
            };
            self.text_m(pm, &shown, PAD + 6.0, ty + 16.0, 12.0, (200, 240, 220), (0.0, h), shape::Mode::Strict);
        }
    }

    /// The "zi" chooser is on screen: rows from the last zi reply, nothing typed, nothing else open.
    fn zi_active(&self) -> bool {
        !self.zi_rows.is_empty() && self.input.text().is_empty() && !self.busy && self.confirm.is_none() && self.expanded
    }

    fn zi_pick(&mut self) {
        if let Some(p) = self.zi_rows.get(self.zi_sel.min(self.zi_rows.len().saturating_sub(1))).cloned() {
            self.input.clear();
            match p.split_once('\t') {
                Some((_, cmd)) => self.input.insert(cmd),
                None => self.input.insert(&format!("list {p}")),
            }
        }
        self.zi_rows.clear();
        self.zi_sel = 0;
    }

    /// (top, rows_height, total_height) of the zi chooser: rows plus one hint line.
    fn zi_geom(&self) -> (f32, f32, f32) {
        let rows_h = self.zi_rows.len() as f32 * 26.0;
        let total = rows_h + 22.0 + 8.0;
        (self.input_box_top - 6.0 - total - 8.0, rows_h, total)
    }

    fn draw_zi(&mut self, pm: &mut Pixmap, w: f32, h: f32) {
        if !self.zi_active() {
            return;
        }
        let rh = 26.0;
        let (top, rows_h, total) = self.zi_geom();
        fill(pm, &rrect(PAD - 2.0, top, w - 2.0 * PAD + 4.0, total, 10.0), (36, 33, 58, 255));
        let sel = self.zi_sel.min(self.zi_rows.len() - 1);
        let rows = self.zi_rows.clone();
        for (k, p) in rows.iter().map(|r| r.split('\t').next().unwrap_or("").to_string()).enumerate() {
            let y = top + 4.0 + k as f32 * rh;
            if k == sel {
                fill(pm, &rrect(PAD + 2.0, y, w - 2.0 * PAD - 4.0, rh - 2.0, 6.0), (92, 80, 170, 255));
            }
            let shown = if p.chars().count() > 44 { format!("...{}", p.chars().rev().take(41).collect::<Vec<_>>().into_iter().rev().collect::<String>()) } else { p.clone() };
            self.text_m(pm, &shown, PAD + 10.0, y + 18.0, 13.0, (255, 255, 255), (0.0, h), shape::Mode::Strict);
        }
        self.text(pm, "Up/Down choose, Tab fills, Esc closes, Enter runs", PAD + 10.0, top + 4.0 + rows_h + 15.0, 11.0, (190, 185, 215), (0.0, h));
    }

    fn draw_menu(&mut self, pm: &mut Pixmap, w: f32, h: f32) {
        let rows = self.menu_rows();
        if rows.is_empty() {
            return;
        }
        let rh = 26.0;
        let n = rows.len() as f32;
        let top = self.input_box_top - 6.0 - n * rh - 8.0;
        fill(pm, &rrect(PAD - 2.0, top, w - 2.0 * PAD + 4.0, n * rh + 8.0, 10.0), (36, 33, 58, 255));
        let sel = self.menu_sel.min(rows.len() - 1);
        for (k, i) in rows.iter().enumerate() {
            let y = top + 4.0 + k as f32 * rh;
            if k == sel {
                fill(pm, &rrect(PAD + 2.0, y, w - 2.0 * PAD - 4.0, rh - 2.0, 6.0), (92, 80, 170, 255));
            }
            let (name, desc, _) = SKILL_MENU[*i];
            self.text(pm, name, PAD + 10.0, y + 18.0, 14.0, (255, 255, 255), (0.0, h));
            self.text(pm, desc, PAD + 100.0, y + 18.0, 12.0, (190, 185, 215), (0.0, h));
        }
    }

    fn submit(&mut self) {
        let draft = self.input.text().trim().to_string();
        if self.busy {
            return;
        }
        // a skill whose starter ends in a space still needs words from the user
        if let Some(s) = self.skill.and_then(|i| SKILL_MENU.get(i)) {
            if draft.is_empty() && s.2.ends_with(' ') {
                self.toast = Some("Add the details for this skill first");
                return;
            }
        }
        let t = compose_request(self.skill, &draft);
        if t.is_empty() {
            return;
        }
        self.input.clear();
        self.skill = None;
        self.zi_rows.clear();
        let rep = self.reply.take();
        let quote = rep.as_ref().map(|r| r.1.clone());
        // Local follow-up that really uses the reply: "copy" copies the quoted message. Nothing is sent anywhere.
        if let Some((full, _)) = &rep {
            if matches!(t.to_lowercase().trim_end_matches(['.', '!']), "copy" | "copy this" | "copy that" | "copy it") {
                self.push(true, "user", t.clone(), quote);
                let ok = self.clip_copy(full.clone());
                self.push(false, "message", if ok { "Copied that message.".into() } else { "No clipboard here, nothing copied.".into() }, None);
                self.draw();
                return;
            }
        }
        self.push(true, "user", t.clone(), quote);
        self.busy = true;
        let _ = self.tx.send(Req::Text(t, self.pill.clone()));
    }

    fn set_reply(&mut self, text: &str) {
        let full = strip_took(text);
        let one: String = full.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut prev: String = one.chars().take(60).collect();
        if one.chars().count() > 60 {
            prev.push_str("...");
        }
        self.reply = Some((full, prev));
        self.draw();
    }

    fn decide(&mut self, yes: bool) {
        if self.busy || self.confirm.is_none() {
            return;
        }
        if yes && !self.can_approve() {
            return;
        }
        self.confirm = None;
        self.plan_scroll = 0.0;
        self.busy = true;
        let _ = self.tx.send(if yes { Req::Confirm } else { Req::Cancel });
    }

    fn on_reply(&mut self, r: UiReply) {
        self.busy = false;
        self.push(false, &r.kind, r.text.clone(), None);
        if r.kind == "md" {
            if let Some(m) = self.msgs.last_mut() {
                m.md = true;
            }
        }
        if voice::should_speak(self.settings.speak, &r.kind) {
            let t = r.text.clone();
            self.speak_reply(&t);
        }
        self.zi_rows = if r.kind == "message" { r.plan.as_deref().map(|p| p.lines().map(String::from).collect()).unwrap_or_default() } else { vec![] };
        self.zi_sel = 0;
        if r.kind == "confirm" {
            self.confirm = Some(r.text);
            self.focus = Btn::No; // Cancel is focused by default
            self.plan_scroll = 0.0;
        }
        self.draw();
    }

    fn key(&mut self, e: KeyEvent) {
        self.toast = None;
        if !matches!(self.vstate, VState::Working) {
            self.vnote = false;
        }
        if matches!(self.vstate, VState::Working) {
            if e.keysym == Keysym::Escape {
                self.cancel_working();
            }
            return;
        }
        if !self.expanded && !self.bar {
            if matches!(e.keysym, Keysym::Return | Keysym::KP_Enter | Keysym::space) {
                self.set_expanded(true);
            }
            return;
        }
        if matches!(self.vstate, VState::Listening { .. }) {
            match e.keysym {
                Keysym::Escape => self.cancel_listening(),
                Keysym::Return | Keysym::KP_Enter => self.stop_listening(),
                _ => {}
            }
            return;
        }
        if self.speaking.is_some() && e.keysym == Keysym::Escape {
            self.stop_speaking();
            self.draw();
            return;
        }
        let page = 200.0;
        if self.confirm.is_some() {
            // Typing never approves. Only Left/Right/Tab, Enter on the
            // focused button, `y` while Do it is focused, `n`, Esc.
            match e.keysym {
                Keysym::Left | Keysym::Right | Keysym::Tab => {
                    self.focus = if self.focus == Btn::Yes { Btn::No } else { Btn::Yes };
                }
                Keysym::Down => self.plan_scroll = (self.plan_scroll + LH).min(self.plan_max),
                Keysym::Up => self.plan_scroll = (self.plan_scroll - LH).max(0.0),
                Keysym::Page_Down => self.plan_scroll = (self.plan_scroll + page).min(self.plan_max),
                Keysym::Page_Up => self.plan_scroll = (self.plan_scroll - page).max(0.0),
                Keysym::Return | Keysym::KP_Enter => {
                    let yes = self.focus == Btn::Yes;
                    self.decide(yes);
                }
                Keysym::Escape => self.decide(false),
                _ => match e.utf8.as_deref() {
                    Some("y") if self.focus == Btn::Yes => self.decide(true),
                    Some("n") => self.decide(false),
                    _ => {}
                },
            }
            self.draw();
            return;
        }
        if self.settings_open {
            let n = self.settings_rows();
            match e.keysym {
                Keysym::Escape => self.settings_open = false,
                Keysym::Up => self.settings_sel = (self.settings_sel + n - 1) % n,
                Keysym::Down | Keysym::Tab => self.settings_sel = (self.settings_sel + 1) % n,
                Keysym::Return | Keysym::KP_Enter => self.apply_setting(self.settings_sel),
                _ => {}
            }
            self.draw();
            return;
        }
        match e.keysym {
            Keysym::r | Keysym::R if self.ctrl && !self.busy => {
                if let Some(t) = self.msgs.last().map(|m| m.text.clone()) {
                    self.set_reply(&t);
                }
            }
            Keysym::Escape if self.pill_active() => {
                self.pill_rows.clear();
                self.pill_off = true;
                self.draw();
            }
            Keysym::Return | Keysym::KP_Enter | Keysym::Tab if self.pill_active() => {
                if !fsearch::can_pick(self.pill_wait, self.pill_rows.len()) {
                    return;
                }
                self.pill_pick();
                self.draw();
            }
            Keysym::Up | Keysym::Down if self.pill_active() => {
                let n = self.pill_rows.len().max(1);
                self.pill_sel = if e.keysym == Keysym::Up { (self.pill_sel + n - 1) % n } else { (self.pill_sel + 1) % n };
                self.draw();
            }
            Keysym::BackSpace if (self.pill.is_some() || self.skill.is_some()) && self.input.caret() == 0 && self.input.sel().is_none() && !self.busy => {
                // chips come off from the right: folder first, then the skill
                if self.pill.take().is_none() {
                    self.skill = None;
                }
                self.pill_show = false;
                self.draw();
            }
            Keysym::Escape if self.zi_active() => {
                self.zi_rows.clear();
                self.draw();
            }
            Keysym::Tab if self.zi_active() => {
                self.zi_pick();
                self.draw();
            }
            Keysym::Up | Keysym::Down if self.zi_active() => {
                let n = self.zi_rows.len();
                self.zi_sel = if e.keysym == Keysym::Up { (self.zi_sel + n - 1) % n } else { (self.zi_sel + 1) % n };
                self.draw();
            }
            Keysym::Escape if bridge::weather_pick_cancel() => {
                self.toast = Some("Cancelled. No forecast was fetched");
                self.draw();
            }
            Keysym::Escape if self.reply.is_some() && !self.busy && self.menu_rows().is_empty() => {
                self.reply = None;
                self.draw();
            }
            Keysym::Escape if self.busy => {
                bridge::request_cancel();
            }
            Keysym::Escape if !self.menu_rows().is_empty() => {
                self.input.clear();
                self.menu_sel = 0;
                self.draw();
            }
            Keysym::Escape => self.set_expanded(false),
            Keysym::Tab if !self.menu_rows().is_empty() => {
                self.menu_pick();
                self.draw();
            }
            Keysym::Up | Keysym::Down if !self.menu_rows().is_empty() => {
                let n = self.menu_rows().len();
                self.menu_sel = if e.keysym == Keysym::Up { (self.menu_sel + n - 1) % n } else { (self.menu_sel + 1) % n };
                self.draw();
            }
            Keysym::Return | Keysym::KP_Enter => {
                // a complete "/skills" runs; a partial "/fi" is completed from the highlighted row first
                let complete = SKILL_MENU.iter().any(|m| m.0 == "/skills" && self.input.text().eq_ignore_ascii_case(m.0));
                if !complete && self.menu_pick() {
                } else if !self.expanded && self.input.text().trim().is_empty() {
                    self.set_expanded(true);
                } else {
                    self.submit();
                    if !self.expanded {
                        // asked from the strip: open the card so the answer and any confirm card are seen
                        self.set_expanded(true);
                    }
                }
                self.draw();
            }
            Keysym::BackSpace => {
                self.input.backspace();
                self.draw();
            }
            Keysym::Delete => {
                self.input.delete();
                self.draw();
            }
            Keysym::Left | Keysym::Right | Keysym::Home | Keysym::End => {
                let (sh, word) = (self.shift, self.ctrl);
                match (e.keysym, word) {
                    (Keysym::Left, false) => self.step_visual(true, sh),
                    (Keysym::Right, false) => self.step_visual(false, sh),
                    (Keysym::Left, true) => self.input.word_left(sh),
                    (Keysym::Right, true) => self.input.word_right(sh),
                    (Keysym::Home, _) => self.input.home(sh),
                    _ => self.input.end(sh),
                }
                self.draw();
            }
            Keysym::c | Keysym::C if self.ctrl => {
                // Ctrl+C: selected input text; with nothing typed or selected (or Shift held), the last result.
                let text = if self.shift || self.input.is_blank() {
                    self.last_result_text().unwrap_or_default()
                } else {
                    self.input.copy_text()
                };
                if !text.is_empty() {
                    self.clip_copy(text);
                }
            }
            Keysym::x | Keysym::X if self.ctrl => {
                if !self.input.is_blank() {
                    let t = self.input.cut();
                    self.clip_copy(t);
                    self.draw();
                }
            }
            Keysym::v | Keysym::V | Keysym::Insert if self.ctrl || (self.shift && e.keysym == Keysym::Insert) => {
                if !self.busy {
                    if let Some(t) = self.clip_paste() {
                        // inserted as sanitised text only; never submits anything
                        self.input.insert(&t);
                    }
                }
                self.draw();
            }
            Keysym::a | Keysym::A if self.ctrl => {
                self.input.select_all();
                self.draw();
            }
            Keysym::Up | Keysym::Down if self.input_lines.len() > 1 => {
                let cl = line_of(&self.input_lines, self.input.caret());
                let to = if e.keysym == Keysym::Up { cl.saturating_sub(1) } else { (cl + 1).min(self.input_lines.len() - 1) };
                let col = self.input.caret() - self.input_lines[cl].0;
                let (ls, le) = self.input_lines[to];
                let sh = self.shift;
                self.input.set_caret((ls + col).min(le), sh);
                self.draw();
            }
            Keysym::Page_Up | Keysym::Up => {
                self.scroll = (self.scroll + page).min(self.chat_max);
                self.draw();
            }
            Keysym::Page_Down | Keysym::Down => {
                self.scroll = (self.scroll - page).max(0.0);
                self.draw();
            }
            _ => {
                if !self.busy && !self.ctrl {
                    if let Some(t) = e.utf8 {
                        if !t.is_empty() && !t.chars().any(|c| c.is_control()) {
                            self.input.insert(&t);
                            self.draw();
                        }
                    }
                }
            }
        }
    }

    /// Character index in the input for a pointer position (line from y, then x within the line).
    fn idx_at(&mut self, x: f32, y: f32) -> usize {
        let target = x - (PAD + 8.0);
        let chars: Vec<char> = self.input.text().chars().collect();
        let n = self.input_lines.len().max(1);
        let vis = n.min(MAX_IN_LINES);
        let k = (((y - self.input_box_top - 6.0 - self.chip_h()) / LH).floor().max(0.0) as usize).min(vis - 1);
        let (ls, le) = self.input_lines.get(self.input_top + k).copied().unwrap_or((0, chars.len()));
        let (ls, le) = (ls.min(chars.len()), le.min(chars.len()));
        let text: String = chars[ls..le].iter().collect();
        let laid = self.shaper.layout(&text, PX, shape::Mode::Auto);
        let off = if laid.rtl { (self.input_w - laid.width).max(0.0) } else { 0.0 };
        ls + laid.index_at(target - off).min(le - ls)
    }

    /// Left/Right move the caret the way the text looks, so inside Arabic text Left goes to the next letter.
    fn step_visual(&mut self, left: bool, extend: bool) {
        if !extend && self.input.sel().is_some() {
            return if left { self.input.left(false) } else { self.input.right(false) };
        }
        let chars: Vec<char> = self.input.text().chars().collect();
        let caret = self.input.caret();
        let k = line_of(&self.input_lines, caret).min(self.input_lines.len().saturating_sub(1));
        let (ls, le) = self.input_lines.get(k).copied().unwrap_or((0, chars.len()));
        let (ls, le) = (ls.min(chars.len()), le.min(chars.len()));
        let text: String = chars[ls..le].iter().collect();
        let laid = self.shaper.layout(&text, PX, shape::Mode::Auto);
        match laid.step(caret.clamp(ls, le) - ls, left) {
            Some(i) => self.input.set_caret(ls + i, extend),
            None => {
                // edge of the visual line: continue into the neighbouring line. The left edge of a right-to-left
                // line is its END, so there Left goes forward in the text and Right goes back.
                let back = left != laid.rtl;
                if back { self.input.left(extend) } else { self.input.right(extend) }
            }
        }
    }

    fn right_click(&mut self) {
        if !self.expanded || self.confirm.is_some() || self.busy {
            return;
        }
        let pos = self.ptr_pos;
        if let Some(t) = self.bubbles.iter().find(|(r, _)| inside(*r, pos)).map(|(_, t)| t.clone()) {
            self.set_reply(&t);
        }
    }

    fn click(&mut self) {
        if !self.expanded {
            self.set_expanded(true);
            return;
        }
        if inside(self.r_collapse, self.ptr_pos) {
            self.set_expanded(false);
            return;
        }
        if self.confirm.is_none() && inside(self.r_gear, self.ptr_pos) {
            self.settings_open = !self.settings_open;
            self.draw();
            return;
        }
        if self.settings_open && self.confirm.is_none() {
            let (top, rh) = self.settings_geom();
            let (px, py) = self.ptr_pos;
            let w = self.w as f32;
            for k in 0..self.settings_rows() {
                let y = top + k as f32 * (rh + 6.0);
                if inside((PAD - 2.0, y, w - 2.0 * PAD + 4.0, rh), (px, py)) {
                    self.settings_sel = k;
                    self.apply_setting(k);
                }
            }
            self.draw();
            return;
        }
        if self.confirm.is_none() {
            let (px, py) = self.ptr_pos;
            let w = self.w as f32;
            if inside(self.r_vstop, self.ptr_pos) && matches!(self.vstate, VState::Listening { .. }) {
                self.stop_listening();
                return;
            }
            if inside(self.r_vcancel, self.ptr_pos) {
                if matches!(self.vstate, VState::Working) {
                    self.cancel_working();
                } else if matches!(self.vstate, VState::Listening { .. }) {
                    self.cancel_listening();
                } else {
                    self.stop_speaking();
                    self.draw();
                }
                return;
            }
            if inside(self.r_mic, self.ptr_pos) {
                match self.vstate {
                    VState::Idle => self.start_listening(),
                    VState::Listening { .. } => self.stop_listening(),
                    VState::Working => {}
                }
                return;
            }
            if self.skill.is_some() && !self.busy && inside(self.r_skill_x, self.ptr_pos) {
                self.skill = None;
                self.draw();
                return;
            }
            if self.pill.is_some() && inside(self.r_pill_name, self.ptr_pos) {
                self.pill_show = !self.pill_show;
                self.draw();
                return;
            }
            if self.pill.is_some() && !self.busy && inside(self.r_pill_x, self.ptr_pos) {
                self.pill_show = false;
                self.pill = None;
                self.draw();
                return;
            }
            if self.pill_active() {
                let (top, rows_h) = self.pill_geom();
                if inside((PAD - 2.0, top, w - 2.0 * PAD + 4.0, rows_h + 8.0), self.ptr_pos) {
                    self.pill_sel = (self.pill_window() + (((py as f32 - top - 4.0) / 26.0).floor().max(0.0) as usize)).min(self.pill_rows.len().saturating_sub(1));
                    self.pill_pick();
                    self.draw();
                    return;
                }
            }
            if self.zi_active() {
                let rh = 26.0;
                let (top, rows_h, _) = self.zi_geom();
                if inside((PAD - 2.0, top, w - 2.0 * PAD + 4.0, rows_h + 8.0), self.ptr_pos) {
                    self.zi_sel = (((py as f32 - top - 4.0) / rh).floor().max(0.0) as usize).min(self.zi_rows.len() - 1);
                    self.zi_pick();
                    self.draw();
                    return;
                }
            }
            // a click on a "/" menu row fills the input with that skill (same as Tab)
            let rows = self.menu_rows();
            if !rows.is_empty() {
                let (rh, n) = (26.0, rows.len() as f32);
                let top = self.input_box_top - 6.0 - n * rh - 8.0;
                if inside((PAD - 2.0, top, w - 2.0 * PAD + 4.0, n * rh + 8.0), self.ptr_pos) {
                    let k = (((py as f32 - top - 4.0) / rh).floor().max(0.0) as usize).min(rows.len() - 1);
                    self.menu_sel = k;
                    self.menu_pick();
                    self.draw();
                    return;
                }
            }
            if inside((PAD - 2.0, self.input_box_top, w - 2.0 * PAD + 4.0, 40.0 + self.input_extra), self.ptr_pos) {
                let i = self.idx_at(px as f32, py as f32);
                self.input.set_caret(i, self.shift);
                self.dragging = true;
                self.toast = None;
                self.draw();
                return;
            }
            if self.reply.is_some() && inside((PAD - 2.0, self.input_box_top - REPLY_H, w - 2.0 * PAD + 4.0, REPLY_H), self.ptr_pos) {
                self.reply = None;
                self.draw();
                return;
            }
            // click a message bubble to copy its text (without the "Took" line)
            let hit = self.bubbles.iter().find(|(r, _)| inside(*r, (px, py))).map(|(_, t)| t.clone());
            if let Some(t) = hit {
                let t = strip_took(&t);
                self.toast = Some(if self.clip_copy(t) { "Copied" } else { "No clipboard" });
                self.draw();
                return;
            }
        }
        if self.confirm.is_some() {
            if inside(self.r_yes, self.ptr_pos) {
                self.focus = Btn::Yes;
                self.decide(true);
            } else if inside(self.r_no, self.ptr_pos) {
                self.focus = Btn::No;
                self.decide(false);
            }
            self.draw();
        }
    }

    fn wheel(&mut self, dy: f32) {
        if !self.expanded {
            return;
        }
        if self.confirm.is_some() {
            self.plan_scroll = (self.plan_scroll + dy).clamp(0.0, self.plan_max);
        } else {
            self.scroll = (self.scroll - dy).clamp(0.0, self.chat_max);
        }
        self.draw();
    }
}

#[cfg(unix)]
impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}
#[cfg(unix)]
impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        self.output.as_mut().unwrap()
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}
#[cfg(unix)]
impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface, _: LayerSurfaceConfigure, _: u32) {
        self.draw();
    }
}
#[cfg(unix)]
impl WindowHandler for App {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.exit = true;
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window, _: WindowConfigure, _: u32) {
        self.draw();
    }
}
#[cfg(unix)]
impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        self.seat.as_mut().unwrap()
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Keyboard && self.kb.is_none() {
            self.kb = self.seat.as_mut().unwrap().get_keyboard(qh, &seat, None).ok();
            if let (Some(m), None) = (&self.ddm, &self.ddev) {
                self.ddev = Some(m.get_data_device(qh, &seat));
            }
        }
        if cap == Capability::Pointer && self.ptr.is_none() {
            self.ptr = self.seat.as_mut().unwrap().get_pointer(qh, &seat).ok();
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, cap: Capability) {
        // Devices can be unplugged and re-plugged: drop the dead object so
        // new_capability binds a fresh one.
        if cap == Capability::Keyboard {
            self.kb = None;
        }
        if cap == Capability::Pointer {
            self.ptr = None;
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}
#[cfg(unix)]
impl KeyboardHandler for App {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, serial: u32, _: &[u32], _: &[Keysym]) {
        self.serial = serial;
    }
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {}
    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, serial: u32, e: KeyEvent) {
        self.serial = serial;
        self.key(e);
    }
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, m: Modifiers, _: u32) {
        self.ctrl = m.ctrl;
        self.shift = m.shift;
    }
}
#[cfg(unix)]
impl PointerHandler for App {
    fn pointer_frame(&mut self, conn: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, evs: &[PointerEvent]) {
        for e in evs {
            self.ptr_pos = e.position;
            match e.kind {
                PointerEventKind::Press { button, .. } => self.ptr_press(button, e.position),
                PointerEventKind::Release { .. } => self.ptr_release(),
                PointerEventKind::Motion { .. } => {
                    if self.ptr_motion(e.position, e.position) {
                        let _ = conn.flush();
                    }
                }
                PointerEventKind::Axis { vertical, .. } => {
                    let d = if vertical.discrete != 0 { vertical.discrete as f32 * 40.0 } else { vertical.absolute as f32 };
                    self.wheel(d);
                }
                _ => {}
            }
        }
    }
}

impl App {
    #[cfg(unix)]
    fn can_drag(&self) -> bool {
        self.xb.is_some() || matches!(self.surf, Some(Surf::Layer(_)))
    }
    #[cfg(windows)]
    fn can_drag(&self) -> bool {
        self.xb.is_some()
    }

    /// Move the orb to the current margins (Wayland layer margins, or the X11 window position).
    fn apply_margin(&mut self) {
        #[cfg(unix)]
        if let Some(Surf::Layer(l)) = &self.surf {
            l.set_margin(self.margin.0, self.margin.1, 0, 0);
        }
        if self.xb.is_some() {
            self.x_place();
        }
    }

    /// X11: window position from the margins (top, right) of the whole screen; the open card docks in the corner.
    fn x_place(&self) {
        if let Some(x) = &self.xb {
            let m = if self.expanded { (12, 12) } else { self.margin };
            let (wx, wy, ww, _wh) = x.workarea();
            x.place(wx + ww - self.w as i32 - m.1, wy + m.0, self.w, self.h);
        }
    }

    /// Shared by Wayland and X11. `button`: 272 left, 273 right, 274 middle.
    fn ptr_press(&mut self, button: u32, pos: (f64, f64)) {
        match button {
            274 if !self.expanded => {
                // middle click on the orb: back to the corner
                self.margin = (12, 12);
                self.apply_margin();
                self.draw();
            }
            273 if !self.expanded => self.toggle_bar(),
            273 => self.right_click(),
            _ if !self.expanded && self.can_drag() => {
                self.orb_drag = Some((pos.0, pos.1, false, self.margin));
            }
            _ => self.click(),
        }
    }

    fn ptr_release(&mut self) {
        if self.orb_drag.is_some() {
            let moved = self.orb_drag.take().is_some_and(|d| d.2);
            // in the strip only the orb end opens the card; the rest just takes typing
            if !moved && (!self.bar || self.ptr_pos.0 < 40.0) {
                self.set_expanded(true);
            }
        } else {
            self.dragging = false;
        }
    }

    /// `pos` is relative to the window; `abs` is what the drag maths uses (Wayland: same as pos, because sway keeps
    /// reporting positions relative to the surface's place at the press; X11: root coordinates).
    fn ptr_motion(&mut self, pos: (f64, f64), abs: (f64, f64)) -> bool {
        self.ptr_pos = pos;
        if let Some((px, py, moved, m0)) = self.orb_drag {
            let (dx, dy) = (abs.0 - px, abs.1 - py);
            if moved || dx.abs() + dy.abs() >= 6.0 {
                self.margin = ((m0.0 + dy.round() as i32).max(0), (m0.1 - dx.round() as i32).max(0));
                self.apply_margin();
                self.draw();
                self.orb_drag = Some((px, py, true, m0));
                return true;
            }
        } else if self.dragging {
            let i = self.idx_at(pos.0 as f32, pos.1 as f32);
            self.input.set_caret(i, true);
            self.draw();
        }
        false
    }

    fn x_event(&mut self, ev: XEv) {
        use XEv;
        if std::env::var_os("PAWLINE_DEBUG").is_some() {
            eprintln!("xev {}", match &ev { XEv::Key { sym, .. } => format!("key {sym:x}"), XEv::Press { button, x, y, .. } => format!("press {button} {x} {y}"), XEv::Release { .. } => "release".into(), #[cfg(windows)] XEv::Text { .. } => "text".into(), XEv::Motion { .. } => "motion".into(), XEv::Expose => "expose".into() });
        }
        match ev {
            XEv::Expose => self.draw(),
            XEv::Key { sym, state } => {
                self.shift = state & 1 != 0;
                self.ctrl = state & 4 != 0;
                let ks = Keysym::new(sym);
                // plain text only: Ctrl/Alt chords and control characters carry no text. (Windows sends typed text as
                // XEv::Text, so a Windows key event never carries text.)
                #[cfg(unix)]
                let utf8 = if self.ctrl || state & 8 != 0 { None } else { ks.key_char().filter(|c| !c.is_control()).map(|c| c.to_string()) };
                #[cfg(windows)]
                let utf8 = None;
                self.key(KeyEvent { time: 0, raw_code: 0, keysym: ks, utf8 });
            }
            XEv::Press { button, x, y, rx, ry, state } => {
                self.shift = state & 1 != 0;
                self.ctrl = state & 4 != 0;
                self.ptr_pos = (x, y);
                if let Some(xb) = &self.xb {
                    xb.focus();
                }
                match button {
                    1 => self.ptr_press(272, (rx, ry)),
                    2 => self.ptr_press(274, (rx, ry)),
                    3 => self.ptr_press(273, (rx, ry)),
                    4 => self.wheel(-40.0),
                    5 => self.wheel(40.0),
                    _ => {}
                }
            }
            #[cfg(windows)]
            XEv::Text { sym, text } => {
                self.key(KeyEvent { time: 0, raw_code: 0, keysym: Keysym(sym), utf8: Some(text) });
            }
            XEv::Release { x, y } => {
                self.ptr_pos = (x, y);
                self.ptr_release();
            }
            XEv::Motion { x, y, rx, ry, state } => {
                self.shift = state & 1 != 0;
                self.ptr_motion((x, y), (rx, ry));
            }
        }
    }
}
fn strip_took(t: &str) -> String {
    match t.rsplit_once("\nTook ") {
        Some((a, b)) if b.ends_with('s') && b.len() < 8 => a.to_string(),
        _ => t.to_string(),
    }
}
const CLIP_MIMES: [&str; 5] = ["text/plain;charset=utf-8", "text/plain", "UTF8_STRING", "TEXT", "STRING"];
const CLIP_MAX: u64 = 64 * 1024;

#[cfg(unix)]
impl DataDeviceHandler for App {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_device::WlDataDevice, _: f64, _: f64, _: &wl_surface::WlSurface) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_device::WlDataDevice) {}
    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_device::WlDataDevice, _: f64, _: f64) {}
    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_device::WlDataDevice) {}
    fn drop_performed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_device::WlDataDevice) {}
}
#[cfg(unix)]
impl DataOfferHandler for App {
    fn source_actions(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &mut DragOffer, _: wl_data_device_manager::DndAction) {}
    fn selected_action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &mut DragOffer, _: wl_data_device_manager::DndAction) {}
}
#[cfg(unix)]
impl DataSourceHandler for App {
    fn accept_mime(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_source::WlDataSource, _: Option<String>) {}
    fn send_request(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_source::WlDataSource, mime: String, fd: WritePipe) {
        if !CLIP_MIMES.contains(&mime.as_str()) {
            return;
        }
        let text = self.clip_text.clone();
        // write off the UI thread so a slow reader can never freeze the window
        std::thread::spawn(move || {
            use std::io::Write;
            let mut fd = fd;
            let _ = fd.write_all(text.as_bytes());
        });
    }
    fn cancelled(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_source::WlDataSource) {
        self.csrc = None;
    }
    fn dnd_dropped(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_source::WlDataSource) {}
    fn dnd_finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_source::WlDataSource) {}
    fn action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_source::WlDataSource, _: wl_data_device_manager::DndAction) {}
}

impl App {
    /// Put text on the system clipboard (CLIPBOARD selection).
    fn clip_copy(&mut self, text: String) -> bool {
        if let Some(x) = &self.xb {
            return x.clip_copy(text);
        }
        #[cfg(unix)]
        {
            self.clip_copy_wl(text)
        }
        #[cfg(windows)]
        {
            false
        }
    }
    #[cfg(unix)]
    fn clip_copy_wl(&mut self, text: String) -> bool {
        let (Some(m), Some(dev)) = (&self.ddm, &self.ddev) else { return false };
        let src = m.create_copy_paste_source(self.qh.as_ref().unwrap(), CLIP_MIMES);
        src.set_selection(dev, self.serial);
        self.csrc = Some(src);
        self.clip_text = text;
        true
    }
    /// Read text from the system clipboard. Never blocks the UI for more than half a second.
    fn clip_paste(&mut self) -> Option<String> {
        if let Some(x) = &self.xb {
            return x.clip_paste();
        }
        #[cfg(unix)]
        {
            self.clip_paste_wl()
        }
        #[cfg(windows)]
        {
            None
        }
    }
    #[cfg(unix)]
    fn clip_paste_wl(&mut self) -> Option<String> {
        if self.csrc.is_some() {
            // we own the selection: a self-read would wait on our own event loop
            return Some(self.clip_text.clone());
        }
        let dev = self.ddev.as_ref()?;
        let offer = dev.data().selection_offer()?;
        let mime = offer.with_mime_types(|ms| CLIP_MIMES.iter().find(|m| ms.iter().any(|x| x == **m)).map(|m| m.to_string()))?;
        let pipe = offer.receive(mime).ok()?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            use std::io::Read;
            let mut buf = Vec::new();
            let _ = pipe.take(CLIP_MAX).read_to_end(&mut buf);
            let _ = tx.send(buf);
        });
        let buf = rx.recv_timeout(std::time::Duration::from_millis(500)).ok()?;
        Some(String::from_utf8_lossy(&buf).into_owned())
    }
    /// Text of the latest reply, without the "Took" line.
    fn last_result_text(&self) -> Option<String> {
        let m = self.msgs.iter().rev().find(|m| !m.user)?;
        let t = match m.text.rsplit_once("\nTook ") {
            Some((a, b)) if b.ends_with('s') && b.len() < 8 => a.to_string(),
            _ => m.text.clone(),
        };
        Some(t)
    }
}

#[cfg(unix)]
impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        self.shm.as_mut().unwrap()
    }
}
#[cfg(unix)]
delegate_compositor!(App);
#[cfg(unix)]
delegate_output!(App);
#[cfg(unix)]
delegate_shm!(App);
#[cfg(unix)]
delegate_seat!(App);
#[cfg(unix)]
delegate_keyboard!(App);
#[cfg(unix)]
delegate_pointer!(App);
#[cfg(unix)]
delegate_layer!(App);
#[cfg(unix)]
delegate_xdg_shell!(App);
#[cfg(unix)]
delegate_xdg_window!(App);
#[cfg(unix)]
delegate_data_device!(App);
#[cfg(unix)]
delegate_registry!(App);
#[cfg(unix)]
impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        self.registry.as_mut().unwrap()
    }
    registry_handlers![OutputState, SeatState];
}

/// The folder Pawline works inside: PAWLINE_ROOT if set, else the home folder.
fn work_root() -> std::path::PathBuf {
    let root = std::env::var_os("PAWLINE_ROOT")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute() && p.is_dir());
    match root {
        Some(r) => r,
        None => {
            eprintln!("Pawline needs a home folder: HOME (or PAWLINE_ROOT) is not set to an existing absolute folder. Nothing was started.");
            std::process::exit(2);
        }
    }
}

/// Terminal mode (--no-ui). Same engine, same validation, same confirmation
/// rules. Only a typed "yes" at a real terminal approves a plan; piped or
/// closed input never approves anything.
fn run_terminal() {
    use std::io::{BufRead, IsTerminal, Write};
    let tty = std::io::stdin().is_terminal();
    let mut st = bridge::new_state(work_root());
    println!("Pawline (terminal mode). Type a command, or 'quit' to leave.");
    if !st.notes.is_empty() {
        let n = st.notes.drain(..).collect::<Vec<_>>().join("\n");
        println!("After an interrupted run I checked the undo journal:\n{n}");
    }
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    loop {
        print!("> ");
        let _ = std::io::stdout().flush();
        let line = match lines.next() {
            Some(Ok(l)) => l,
            _ => break,
        };
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t == "quit" || t == "exit" {
            break;
        }
        st.pending = None;
        let r = bridge::handle_text(&mut st, t);
        let r = bridge::with_notes(&mut st, r);
        println!("{}", r.text);
        if r.kind == "confirm" {
            if !tty {
                let _ = bridge::cancel(&mut st);
                println!("Not approved: confirmation needs a person typing at a terminal. Nothing was done.");
                continue;
            }
            print!("Type yes to do this, anything else cancels: ");
            let _ = std::io::stdout().flush();
            let ans = match lines.next() {
                Some(Ok(l)) => l,
                _ => String::new(),
            };
            let r2 = if ans.trim() == "yes" {
                let r = bridge::confirm_inner(&mut st);
                bridge::with_notes(&mut st, r)
            } else {
                bridge::cancel(&mut st)
            };
            println!("{}", r2.text);
        }
    }
}

fn worker(rx: mpsc::Receiver<Req>, back: channel::Sender<UiReply>) {
    let root = work_root();
    let mut st = bridge::new_state(root);
    // Recovery notes from the previous run are shown first.
    if !st.notes.is_empty() {
        let n = st.notes.drain(..).collect::<Vec<_>>().join("\n");
        let _ = back.send(bridge::reply("error", format!("After an interrupted run I checked the undo journal:\n{n}")));
    }
    while let Ok(req) = rx.recv() {
        let r = match req {
            Req::Text(t, dir) => {
                st.pending = None;
                let r = bridge::handle_text_in(&mut st, &t, dir.as_deref());
                bridge::with_notes(&mut st, r)
            }
            Req::Confirm => {
                let r = bridge::confirm_inner(&mut st);
                bridge::with_notes(&mut st, r)
            }
            Req::Cancel => bridge::cancel(&mut st),
        };
        bridge::ONLINE_CHAT_UI.store(st.online_chat, std::sync::atomic::Ordering::Relaxed);
        if back.send(r).is_err() {
            break;
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("pawline [--no-ui] [--panel-only]\n  (no flag)  floating orb and chat (Wayland, or X11 when there is no Wayland display; PAWLINE_BACKEND=x11 forces X11)\n  --no-ui    terminal mode, same commands and confirmations\n  --panel-only  no orb: Pawline stays hidden until a panel widget opens it\n  --ctl toggle|show|hide|status  talk to a running Pawline (toggle/show start it if it is not running)\n  --waybar | --polybar | --genmon  print the panel widget text (see integrations/)\nPAWLINE_NO_LAYER=1 uses a normal window. PAWLINE_ROOT sets the working folder (default: home).");
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--ctl") {
        let word = args.get(i + 1).map(|s| s.as_str()).unwrap_or("");
        if !matches!(word, "toggle" | "show" | "hide" | "status") {
            eprintln!("usage: pawline --ctl toggle|show|hide|status");
            std::process::exit(2);
        }
        match ctl::ask(word) {
            Ok(r) => println!("{r}"),
            Err(_) if matches!(word, "toggle" | "show") => {
                // not running yet: a panel click starts Pawline with its card open, in panel-only mode
                if let Ok(exe) = std::env::current_exe() {
                    let _ = std::process::Command::new(exe)
                        .args(["--panel-only", "--open"])
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                }
            }
            Err(_) => println!("off"),
        }
        return;
    }
    if let Some(fmt) = args.iter().find(|a| matches!(a.as_str(), "--waybar" | "--polybar" | "--genmon")) {
        let st = ctl::ask("status").unwrap_or_else(|_| "off".into());
        println!(
            "{}",
            match fmt.as_str() {
                "--waybar" => ctl::waybar(&st),
                "--polybar" => ctl::polybar(&st),
                _ => ctl::genmon(&st),
            }
        );
        return;
    }
    if args.iter().any(|a| a == "--no-ui") {
        run_terminal();
        return;
    }
    #[cfg(unix)]
    let x_mode = std::env::var("PAWLINE_BACKEND").is_ok_and(|v| v == "x11") || std::env::var_os("WAYLAND_DISPLAY").is_none();
    #[cfg(windows)]
    let x_mode = true; // the Windows window backend plays the role of the X11 one (see winb.rs)
    #[cfg(unix)]
    let no_display = x_mode && std::env::var_os("DISPLAY").is_none();
    #[cfg(windows)]
    let no_display = false;
    if no_display {
        eprintln!("No Wayland or X11 display found, so the window can't open. Starting terminal mode (same commands, same confirmations). Use --no-ui to choose this directly.");
        run_terminal();
        return;
    }
    if ctl::running() {
        eprintln!("Pawline is already running. Use: pawline --ctl show");
        return;
    }
    let panel_only = args.iter().any(|a| a == "--panel-only");
    let state: ctl::Shared = std::sync::Arc::new(std::sync::Mutex::new("idle"));
    #[cfg(unix)]
    struct Wl {
        conn: Connection,
        queue: smithay_client_toolkit::reexports::client::EventQueue<App>,
        globals: smithay_client_toolkit::reexports::client::globals::GlobalList,
        shm: Shm,
        pool: SlotPool,
        surf: Surf,
        layer_mode: bool,
    }
    #[cfg(unix)]
    let wl: Option<Wl> = if x_mode {
        None
    } else {
        let conn = Connection::connect_to_env().expect("no Wayland compositor found (WAYLAND_DISPLAY)");
        let (globals, queue) = registry_queue_init(&conn).unwrap();
        let qh = queue.handle();
        let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor missing");
        let shm = Shm::bind(&globals, &qh).expect("wl_shm missing");
        let force_window = std::env::var("PAWLINE_NO_LAYER").is_ok();
        let layer_shell = if force_window { None } else { LayerShell::bind(&globals, &qh).ok() };
        let surface = compositor.create_surface(&qh);
        let (surf, layer_mode) = match layer_shell {
            Some(ls) => {
                let l = ls.create_layer_surface(&qh, surface, Layer::Top, Some("pawline"), None);
                l.set_anchor(Anchor::TOP | Anchor::RIGHT);
                l.set_margin(12, 12, 0, 0);
                l.set_size(COMPACT, COMPACT);
                l.set_keyboard_interactivity(KeyboardInteractivity::OnDemand);
                l.commit();
                (Surf::Layer(l), true)
            }
            None => {
                let xdg = XdgShell::bind(&globals, &qh).expect("neither wlr-layer-shell nor xdg-shell is available");
                let win = xdg.create_window(surface, WindowDecorations::RequestServer, &qh);
                win.set_title("Pawline");
                win.set_app_id("pawline");
                win.set_min_size(Some((COMPACT, COMPACT)));
                win.set_max_size(Some((COMPACT, COMPACT)));
                win.commit();
                (Surf::Win(win), false)
            }
        };
        let pool = SlotPool::new((W * H * 4) as usize, &shm).unwrap();
        Some(Wl { conn, queue, globals, shm, pool, surf, layer_mode })
    };
    let (x_tx, x_rx) = channel::channel::<XEv>();
    let xb = if x_mode {
        match Xb::open(COMPACT as u16, COMPACT as u16, x_tx) {
            Ok(x) => Some(x),
            Err(e) => {
                eprintln!("Could not open the X11 display ({e}). Starting terminal mode (same commands, same confirmations).");
                run_terminal();
                return;
            }
        }
    } else {
        None
    };

    let (req_tx, req_rx) = mpsc::channel::<Req>();
    let (back_tx, back_rx) = channel::channel::<UiReply>();
    let (voice_tx, voice_rx) = channel::channel::<VEv>();
    let (search_tx, search_rx) = channel::channel::<(u64, String, Vec<std::path::PathBuf>)>();
    std::thread::spawn(move || worker(req_rx, back_tx));

    #[cfg(unix)]
    let (conn_opt, queue_opt, mut wl_rest) = match wl {
        Some(w) => (Some(w.conn), Some(w.queue), Some((w.globals, w.shm, w.pool, w.surf, w.layer_mode))),
        None => (None, None, None),
    };
    #[cfg(unix)]
    let layer_mode = wl_rest.as_ref().is_some_and(|r| r.4);
    #[cfg(windows)]
    let layer_mode = false;
    #[cfg(unix)]
    let qh0 = queue_opt.as_ref().map(|q| q.handle());
    let mut app = App {
        #[cfg(unix)]
        registry: wl_rest.as_ref().map(|r| RegistryState::new(&r.0)),
        #[cfg(unix)]
        seat: wl_rest.as_ref().zip(qh0.as_ref()).map(|(r, q)| SeatState::new(&r.0, q)),
        #[cfg(unix)]
        output: wl_rest.as_ref().zip(qh0.as_ref()).map(|(r, q)| OutputState::new(&r.0, q)),
        #[cfg(unix)]
        shm: None,
        #[cfg(unix)]
        pool: None,
        #[cfg(unix)]
        surf: None,
        #[cfg(unix)]
        qh: qh0.clone(),
        xb,
        #[cfg(unix)]
        ddm: wl_rest.as_ref().zip(qh0.as_ref()).and_then(|(r, q)| DataDeviceManagerState::bind(&r.0, q).ok()),
        #[cfg(unix)]
        ddev: None,
        #[cfg(unix)]
        csrc: None,
        #[cfg(unix)]
        clip_text: String::new(),
        #[cfg(unix)]
        serial: 0,
        #[cfg(unix)]
        kb: None,
        #[cfg(unix)]
        ptr: None,
        w: COMPACT,
        h: COMPACT,
        expanded: false,
        input: edit::Edit::default(),
        ctrl: false,
        shift: false,
        input_top: 0,
        input_extra: 0.0,
        input_box_top: 0.0,
        input_lines: vec![(0, 0)],
        dragging: false,
        bar: false,
        orb_drag: None,
        margin: (12, 12),
        bubbles: Vec::new(),
        toast: None,
        reply: None,
        msgs: vec![Msg { user: false, kind: "message".into(), md: true, text: "Hi, I am Pawline. Try: make a folder called kitten paws. Click any reply to copy it. Right-click a message (or Ctrl+R) to reply to it.".into(), thumb: None, quote: None }],
        confirm: None,
        focus: Btn::No,
        scroll: 0.0,
        plan_scroll: 0.0,
        busy: false,
        menu_sel: 0,
        zi_rows: vec![],
        zi_sel: 0,
        shaper: shape::Shaper::new(),
        pill: None,
        root: work_root(),
        pill_off: false,
        pill_rows: vec![],
        pill_sel: 0,
        pill_q: None,
        skill: None,
        r_skill_x: (0.0, 0.0, 0.0, 0.0),
        r_skill_name: (0.0, 0.0, 0.0, 0.0),
        pill_gen: 0,
        stt_cmd: None,
        vjob: 0,
        vcancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        vnote: false,
        vstate: VState::Idle,
        speaking: None,
        r_mic: (0.0, 0.0, 0.0, 0.0),
        r_vstop: (0.0, 0.0, 0.0, 0.0),
        r_vcancel: (0.0, 0.0, 0.0, 0.0),
        pill_wait: false,
        pill_show: false,
        r_pill_name: (0.0, 0.0, 0.0, 0.0),
        r_pill_x: (0.0, 0.0, 0.0, 0.0),
        settings: settings_path().map(|p| theme::load(&p)).unwrap_or_default(),
        settings_open: false,
        settings_sel: 0,
        t0: std::time::Instant::now(),
        r_gear: (0.0, 0.0, 0.0, 0.0),
        input_w: 300.0,
        tx: req_tx,
        voice_tx: Some(voice_tx),
        searcher: {
            let root = work_root();
            let tx = search_tx.clone();
            let finder: fsearch::Finder = std::sync::Arc::new(move |q, cancel| bridge::folder_candidates(&root, q, cancel));
            let deliver: fsearch::Deliver = std::sync::Arc::new(move |g, q, rows| {
                let _ = tx.send((g, q, rows));
            });
            Some(fsearch::Searcher::new(finder, deliver, std::time::Duration::from_millis(120)))
        },
        exit: false,
        panel_only,
        hidden: panel_only,
        state: state.clone(),
        layer_mode,
        r_yes: (0.0, 0.0, 0.0, 0.0),
        r_no: (0.0, 0.0, 0.0, 0.0),
        r_collapse: (0.0, 0.0, 0.0, 0.0),
        ptr_pos: (0.0, 0.0),
        plan_max: 0.0,
        chat_max: 0.0,
    };
    #[cfg(unix)]
    if let Some(r) = wl_rest.take() {
        app.shm = Some(r.1);
        app.pool = Some(r.2);
        app.surf = Some(r.3);
    }
    let (ctl_tx, ctl_rx) = channel::channel::<ctl::Ctl>();
    if !ctl::serve(ctl_tx, state.clone()) {
        eprintln!("(panel control socket not available; panel widgets will show Pawline as off)");
    }
    if args.iter().any(|a| a == "--open") {
        app.expanded = true;
        app.hidden = false;
        app.w = W;
        app.h = H;
        #[cfg(unix)]
        if let Some(Surf::Layer(l)) = &app.surf {
            l.set_size(W, H);
        }
    }
    if std::env::var("PAWLINE_START_EXPANDED").is_ok() {
        app.expanded = true;
        app.w = W;
        app.h = H;
        #[cfg(unix)]
        if let Some(Surf::Layer(l)) = &app.surf {
            l.set_size(W, H);
        }
    }

    let mut event_loop: EventLoop<App> = EventLoop::try_new().unwrap();
    let handle = event_loop.handle();
    #[cfg(unix)]
    if let (Some(c), Some(q)) = (conn_opt.clone(), queue_opt) {
        WaylandSource::new(c, q).insert(handle.clone()).unwrap();
    }
    handle
        .insert_source(x_rx, |ev, _, app: &mut App| {
            if let channel::Event::Msg(e) = ev {
                app.x_event(e);
            }
        })
        .unwrap();
    handle
        .insert_source(voice_rx, |ev, _, app: &mut App| {
            if let channel::Event::Msg(e) = ev {
                app.on_voice(e);
            }
        })
        .unwrap();
    handle
        .insert_source(search_rx, |ev, _, app: &mut App| {
            if let channel::Event::Msg((g, q, rows)) = ev {
                app.on_search(g, q, rows);
            }
        })
        .unwrap();
    handle
        .insert_source(back_rx, |ev, _, app: &mut App| {
            if let channel::Event::Msg(r) = ev {
                app.on_reply(r);
            }
        })
        .unwrap();
    handle
        .insert_source(ctl_rx, |ev, _, app: &mut App| {
            if let channel::Event::Msg(c) = ev {
                app.on_ctl(c);
            }
        })
        .unwrap();
    // the background colours drift slowly: redraw about once a second, only while the card is open and drift is on
    handle
        .insert_source(
            calloop::timer::Timer::from_duration(std::time::Duration::from_millis(1000)),
            |_, _, app: &mut App| {
                if app.settings.drift && app.expanded && !app.hidden && app.confirm.is_none() {
                    app.draw();
                }
                app.voice_tick();
                calloop::timer::TimeoutAction::ToDuration(std::time::Duration::from_millis(1000))
            },
        )
        .unwrap();
    // debug builds only: show a Markdown sample file as a reply (for screenshots)
    #[cfg(debug_assertions)]
    if let Some(f) = std::env::var_os("PAWLINE_DEMO_MD") {
        if let Ok(t) = std::fs::read_to_string(f) {
            app.push(false, "md", t, None);
            if let Some(m) = app.msgs.last_mut() {
                m.md = true;
            }
        }
    }
    if app.xb.is_some() {
        app.draw();
    }
    while !app.exit {
        event_loop.dispatch(None, &mut app).unwrap();
    }
}

#[cfg(test)]
mod wrap_tests {
    use super::*;
    #[test]
    fn path_lines_are_laid_out_strictly_and_prose_is_not() {
        assert_eq!(line_mode("1. Move /tmp/ah/Downloads/\u{62a}\u{642}.txt to"), shape::Mode::Strict);
        assert_eq!(line_mode("~/Documents/\u{62a}"), shape::Mode::Strict);
        assert_eq!(line_mode("/home/u/x"), shape::Mode::Strict);
        assert_eq!(line_mode("\u{645}\u{631}\u{62d}\u{628}\u{627} \u{628}\u{627}\u{644}\u{639}\u{627}\u{644}\u{645}"), shape::Mode::Auto);
        assert_eq!(line_mode("open Downloads"), shape::Mode::Auto);
    }

    #[test]
    fn wraps_by_width_and_places_caret() {
        let w = vec![10.0; 25];
        let no = vec![false; 25];
        let l = wrap_ranges(&w, 100.0, &no);
        assert_eq!(l, vec![(0, 10), (10, 20), (20, 25)]);
        assert_eq!(wrap_ranges(&[], 100.0, &[]), vec![(0, 0)]);
        assert_eq!(line_of(&l, 0), 0);
        assert_eq!(line_of(&l, 10), 1); // boundary belongs to the next line
        assert_eq!(line_of(&l, 25), 2); // end of text stays on the last line
        // a glyph wider than the box still advances
        assert_eq!(wrap_ranges(&[500.0, 5.0], 100.0, &[false, false]), vec![(0, 1), (1, 2)]);
    }

    #[test]
    fn click_x_maps_to_nearest_glyph_boundary() {
        let w = vec![10.0; 5];
        assert_eq!(pick_in_line(&w, -5.0), 0);
        assert_eq!(pick_in_line(&w, 4.0), 0);
        assert_eq!(pick_in_line(&w, 6.0), 1);
        assert_eq!(pick_in_line(&w, 24.0), 2);
        assert_eq!(pick_in_line(&w, 500.0), 5);
    }

    #[test]
    fn breaks_after_a_space_when_there_is_one() {
        // "aaa bbbbbbb" at 10 px per char, 80 px wide: break after the space, not inside the word
        let t: Vec<char> = "aaa bbbbbbb".chars().collect();
        let w = vec![10.0; t.len()];
        let sp: Vec<bool> = t.iter().map(|c| *c == ' ').collect();
        assert_eq!(wrap_ranges(&w, 80.0, &sp), vec![(0, 4), (4, 11)]);
        // ranges always cover every char exactly once
        let l = wrap_ranges(&w, 30.0, &sp);
        assert_eq!(l.first().unwrap().0, 0);
        assert_eq!(l.last().unwrap().1, t.len());
        assert!(l.windows(2).all(|p| p[0].1 == p[1].0));
    }
}

#[cfg(test)]
mod saved_tests {
    use super::*;
    #[test]
    fn only_pictures_pawline_files_get_a_preview() {
        let ok = "Done:\nMake a picture\nSaved /home/u/Pictures/Pawline/pawline-1-pollinations.png\nTook 0.6s";
        assert!(saved_picture(ok).is_some());
        assert!(saved_picture("Saved /etc/passwd").is_none());
        assert!(saved_picture("Saved /home/u/Pictures/other/a.png").is_none());
        assert!(saved_picture("Saved Pictures/Pawline/a.png").is_none());
        assert!(saved_picture("no path here").is_none());
    }
}
