//! Pawline native UI. Pure CPU rendering (tiny-skia + fontdue) into wl_shm
//! buffers on a Wayland layer-shell surface (xdg-toplevel fallback). No
//! OpenGL, EGL, GTK or WebKit is loaded.
mod bridge;
mod edit;

use bridge::UiReply;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer,
    delegate_registry, delegate_seat, delegate_shm, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{self, channel, EventLoop},
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
use std::collections::HashMap;
use std::sync::mpsc;
use tiny_skia::*;

static FONT: &[u8] = include_bytes!("../assets/font.ttf");

const W: u32 = 440;
const H: u32 = 470;
const COMPACT: u32 = 68;
const PX: f32 = 15.0;
const LH: f32 = 20.0;
const PAD: f32 = 14.0;
const CHAT_TOP: f32 = 64.0;
const BOTTOM: f32 = 64.0;

enum Req {
    Text(String),
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
}

enum Surf {
    Layer(LayerSurface),
    Win(Window),
}

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
    registry: RegistryState,
    seat: SeatState,
    output: OutputState,
    shm: Shm,
    pool: SlotPool,
    surf: Surf,
    qh: QueueHandle<App>,
    kb: Option<wl_keyboard::WlKeyboard>,
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
    font: fontdue::Font,
    cache: HashMap<(char, u32), (fontdue::Metrics, Vec<u8>)>,
    tx: mpsc::Sender<Req>,
    exit: bool,
    layer_mode: bool,
    // hit rects (x, y, w, h), set by draw
    r_yes: (f32, f32, f32, f32),
    r_no: (f32, f32, f32, f32),
    r_collapse: (f32, f32, f32, f32),
    ptr_pos: (f64, f64),
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
    fn glyph(&mut self, ch: char, px: f32) -> &(fontdue::Metrics, Vec<u8>) {
        let key = (ch, px.to_bits());
        if !self.cache.contains_key(&key) {
            let g = self.font.rasterize(ch, px);
            self.cache.insert(key, g);
        }
        &self.cache[&key]
    }

    fn adv(&mut self, ch: char, px: f32) -> f32 {
        self.glyph(ch, px).0.advance_width
    }

    /// Word-wrap `text` to `maxw` pixels. Long words are split. Newlines kept.
    fn wrap(&mut self, text: &str, px: f32, maxw: f32) -> Vec<String> {
        let mut out = Vec::new();
        for para in text.split('\n') {
            let mut line = String::new();
            let mut lw = 0.0;
            for word in para.split(' ') {
                let ww: f32 = word.chars().map(|c| self.adv(c, px)).sum();
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
        let mut cx = x;
        let (pw, ph) = (pm.width() as i32, pm.height() as i32);
        for ch in s.chars() {
            let (m, bm) = {
                let g = self.glyph(ch, px);
                (g.0, g.1.clone())
            };
            let gy = base_y as i32 - m.height as i32 - m.ymin;
            for yy in 0..m.height as i32 {
                let Y = gy + yy;
                if Y < 0 || Y >= ph || (Y as f32) < clip.0 || (Y as f32) >= clip.1 {
                    continue;
                }
                for xx in 0..m.width as i32 {
                    let a = bm[(yy as usize) * m.width + xx as usize] as u32;
                    let X = cx as i32 + m.xmin + xx;
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
            cx += m.advance_width;
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
        let (w, h) = (self.w, self.h);
        let mut pm = Pixmap::new(w, h).unwrap();
        if !self.expanded {
            self.orb(&mut pm, w as f32 / 2.0, h as f32 / 2.0, 24.0, self.busy);
        } else {
            self.draw_card(&mut pm);
        }
        let stride = w as i32 * 4;
        let (buffer, canvas) = self
            .pool
            .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888)
            .unwrap();
        // tiny-skia: premultiplied RGBA; wl_shm Argb8888 (LE): B,G,R,A premultiplied
        for (d, s) in canvas.chunks_exact_mut(4).zip(pm.data().chunks_exact(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
        self.surf.wl().damage_buffer(0, 0, w as i32, h as i32);
        buffer.attach_to(self.surf.wl()).unwrap();
        self.surf.commit();
    }

    fn draw_card(&mut self, pm: &mut Pixmap) {
        let (w, h) = (self.w as f32, self.h as f32);
        fill(pm, &rrect(0.0, 0.0, w, h, 18.0), (30, 28, 40, 245));
        self.orb(pm, 34.0, 32.0, 17.0, self.busy);
        let title = if self.busy { "Pawline  (thinking...)" } else { "Pawline" };
        self.text(pm, title, 62.0, 38.0, 18.0, (235, 230, 250), (0.0, h));
        // collapse button
        self.r_collapse = (w - 44.0, 14.0, 30.0, 30.0);
        fill(pm, &rrect(self.r_collapse.0, self.r_collapse.1, 30.0, 30.0, 8.0), (52, 48, 70, 255));
        self.text(pm, "-", w - 33.0, 36.0, 20.0, (220, 215, 235), (0.0, h));

        let inner_w = w - 2.0 * PAD;
        if self.confirm.is_some() {
            self.draw_confirm(pm, w, h, inner_w);
        } else {
            self.draw_chat(pm, w, h, inner_w);
            // input box
            fill(pm, &rrect(PAD - 2.0, h - 52.0, w - 2.0 * PAD + 4.0, 40.0, 12.0), (52, 48, 70, 255));
            let (mut before, mut after) = self.input.split();
            // keep the caret visible: trim from the left first, then from the right
            while before.chars().chain(after.chars()).map(|c| self.adv(c, PX)).sum::<f32>() > inner_w - 20.0 {
                if !before.is_empty() {
                    before.remove(0);
                } else if !after.is_empty() {
                    after.pop();
                } else {
                    break;
                }
            }
            let shown = format!("{before}|{after}");
            let col = if self.busy { (150, 145, 165) } else { (255, 255, 255) };
            self.text(pm, &shown, PAD + 8.0, h - 26.0, PX, col, (0.0, h));
        }
    }

    fn draw_chat(&mut self, pm: &mut Pixmap, w: f32, h: f32, inner_w: f32) {
        let top = CHAT_TOP;
        let bot = h - BOTTOM + 4.0;
        // layout all messages
        let bub_w = inner_w - 40.0;
        let mut items: Vec<(bool, String, Vec<String>, f32)> = Vec::new();
        let mut total = 0.0;
        let msgs: Vec<(bool, String, String)> = self.msgs.iter().map(|m| (m.user, m.kind.clone(), m.text.clone())).collect();
        for (user, kind, text) in msgs {
            let lines = self.wrap(&text, PX, bub_w - 20.0);
            let bh = lines.len() as f32 * LH + 14.0;
            total += bh + 8.0;
            items.push((user, kind, lines, bh));
        }
        let view = bot - top;
        self.chat_max = (total - view).max(0.0);
        if self.scroll > self.chat_max {
            self.scroll = self.chat_max;
        }
        // y of content start so that the bottom aligns with `bot` + scroll
        let mut cp = Pixmap::new(w as u32, view as u32).unwrap();
        let mut y = view - total + self.scroll;
        for (user, kind, lines, bh) in items {
            let bw = lines.iter().map(|l| l.chars().map(|c| self.adv(c, PX)).sum::<f32>()).fold(0.0, f32::max) + 20.0;
            let bx = if user { w - PAD - bw } else { PAD };
            if y + bh > 0.0 && y < view {
                let col = if user {
                    (62, 54, 112, 255)
                } else if kind == "error" || kind == "failed" {
                    (92, 40, 52, 255)
                } else {
                    (46, 42, 64, 255)
                };
                fill(&mut cp, &rrect(bx, y, bw, bh, 12.0), col);
                for (i, l) in lines.iter().enumerate() {
                    self.text(&mut cp, l, bx + 10.0, y + 7.0 + (i as f32 + 1.0) * LH - 5.0, PX, (235, 232, 245), (0.0, view));
                }
            }
            y += bh + 8.0;
        }
        pm.draw_pixmap(0, top as i32, cp.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        if self.chat_max > 0.0 && self.scroll < self.chat_max - 1.0 {
            fill(pm, &rrect(PAD, top + 2.0, 190.0, 18.0, 8.0), (30, 28, 40, 230));
            self.text(pm, "^ older (PageUp / wheel)", PAD + 6.0, top + 15.0, 12.0, (170, 165, 195), (0.0, h));
        }
        if self.scroll > 1.0 {
            fill(pm, &rrect(w - 160.0, bot - 18.0, 146.0, 18.0, 8.0), (30, 28, 40, 230));
            self.text(pm, "v newer (PageDown)", w - 154.0, bot - 5.0, 12.0, (170, 165, 195), (0.0, h));
        }
    }

    fn draw_confirm(&mut self, pm: &mut Pixmap, w: f32, h: f32, inner_w: f32) {
        let text = self.confirm.clone().unwrap();
        let top = CHAT_TOP;
        let bot = h - BOTTOM - 8.0;
        let panel = rrect(PAD - 4.0, top, w - 2.0 * PAD + 8.0, bot - top, 14.0);
        fill(pm, &panel, (29, 20, 64, 255));
        stroke(pm, &panel, (124, 58, 237, 255), 1.5);
        let lines = self.wrap(&text, PX, inner_w - 16.0);
        let total = lines.len() as f32 * LH + 16.0;
        let view = bot - top - 6.0;
        self.plan_max = (total - view).max(0.0);
        if self.plan_scroll > self.plan_max {
            self.plan_scroll = self.plan_max;
        }
        for (i, l) in lines.iter().enumerate() {
            let y = top + 8.0 + (i as f32 + 1.0) * LH - 5.0 - self.plan_scroll;
            self.text(pm, l, PAD + 4.0, y, PX, (235, 230, 255), (top + 3.0, bot - 3.0));
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

    fn set_expanded(&mut self, exp: bool) {
        self.expanded = exp;
        if exp {
            self.w = W;
            self.h = H;
        } else {
            self.w = COMPACT;
            self.h = COMPACT;
        }
        match &self.surf {
            Surf::Layer(l) => l.set_size(self.w, self.h),
            Surf::Win(win) => {
                win.set_min_size(Some((self.w, self.h)));
                win.set_max_size(Some((self.w, self.h)));
            }
        }
        self.draw();
    }

    fn push(&mut self, user: bool, kind: &str, text: String) {
        self.msgs.push(Msg { user, kind: kind.into(), text });
        self.scroll = 0.0;
    }

    fn submit(&mut self) {
        let t = self.input.text().trim().to_string();
        if t.is_empty() || self.busy {
            return;
        }
        self.input.clear();
        self.push(true, "user", t.clone());
        self.busy = true;
        let _ = self.tx.send(Req::Text(t));
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
        self.push(false, &r.kind, r.text.clone());
        if r.kind == "confirm" {
            self.confirm = Some(r.text);
            self.focus = Btn::No; // Cancel is focused by default
            self.plan_scroll = 0.0;
        }
        self.draw();
    }

    fn key(&mut self, e: KeyEvent) {
        if !self.expanded {
            if matches!(e.keysym, Keysym::Return | Keysym::KP_Enter | Keysym::space) {
                self.set_expanded(true);
            }
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
        match e.keysym {
            Keysym::Escape => self.set_expanded(false),
            Keysym::Return | Keysym::KP_Enter => {
                self.submit();
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
                    (Keysym::Left, false) => self.input.left(sh),
                    (Keysym::Right, false) => self.input.right(sh),
                    (Keysym::Left, true) => self.input.word_left(sh),
                    (Keysym::Right, true) => self.input.word_right(sh),
                    (Keysym::Home, _) => self.input.home(sh),
                    _ => self.input.end(sh),
                }
                self.draw();
            }
            Keysym::a | Keysym::A if self.ctrl => {
                self.input.select_all();
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

    fn click(&mut self) {
        if !self.expanded {
            self.set_expanded(true);
            return;
        }
        if inside(self.r_collapse, self.ptr_pos) {
            self.set_expanded(false);
            return;
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

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}
impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}
impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface, _: LayerSurfaceConfigure, _: u32) {
        self.draw();
    }
}
impl WindowHandler for App {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.exit = true;
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window, _: WindowConfigure, _: u32) {
        self.draw();
    }
}
impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Keyboard && self.kb.is_none() {
            self.kb = self.seat.get_keyboard(qh, &seat, None).ok();
        }
        if cap == Capability::Pointer && self.ptr.is_none() {
            self.ptr = self.seat.get_pointer(qh, &seat).ok();
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
impl KeyboardHandler for App {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {}
    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, e: KeyEvent) {
        self.key(e);
    }
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, m: Modifiers, _: u32) {
        self.ctrl = m.ctrl;
        self.shift = m.shift;
    }
}
impl PointerHandler for App {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, evs: &[PointerEvent]) {
        for e in evs {
            self.ptr_pos = e.position;
            match e.kind {
                PointerEventKind::Press { .. } => self.click(),
                PointerEventKind::Axis { vertical, .. } => {
                    let d = if vertical.discrete != 0 { vertical.discrete as f32 * 40.0 } else { vertical.absolute as f32 };
                    self.wheel(d);
                }
                _ => {}
            }
        }
    }
}
impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}
delegate_compositor!(App);
delegate_output!(App);
delegate_shm!(App);
delegate_seat!(App);
delegate_keyboard!(App);
delegate_pointer!(App);
delegate_layer!(App);
delegate_xdg_shell!(App);
delegate_xdg_window!(App);
delegate_registry!(App);
impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
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
            Req::Text(t) => {
                st.pending = None;
                let r = bridge::handle_text(&mut st, &t);
                bridge::with_notes(&mut st, r)
            }
            Req::Confirm => {
                let r = bridge::confirm_inner(&mut st);
                bridge::with_notes(&mut st, r)
            }
            Req::Cancel => bridge::cancel(&mut st),
        };
        if back.send(r).is_err() {
            break;
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("pawline [--no-ui]\n  (no flag)  floating orb and chat on Wayland\n  --no-ui    terminal mode, same commands and confirmations\nPAWLINE_NO_LAYER=1 uses a normal window. PAWLINE_ROOT sets the working folder (default: home).");
        return;
    }
    if args.iter().any(|a| a == "--no-ui") {
        run_terminal();
        return;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        eprintln!("No Wayland display found, so the window can't open. Starting terminal mode (same commands, same confirmations). Use --no-ui to choose this directly.");
        run_terminal();
        return;
    }
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
    let font = fontdue::Font::from_bytes(FONT, fontdue::FontSettings::default()).unwrap();

    let (req_tx, req_rx) = mpsc::channel::<Req>();
    let (back_tx, back_rx) = channel::channel::<UiReply>();
    std::thread::spawn(move || worker(req_rx, back_tx));

    let mut app = App {
        registry: RegistryState::new(&globals),
        seat: SeatState::new(&globals, &qh),
        output: OutputState::new(&globals, &qh),
        shm,
        pool,
        surf,
        qh: qh.clone(),
        kb: None,
        ptr: None,
        w: COMPACT,
        h: COMPACT,
        expanded: false,
        input: edit::Edit::default(),
        ctrl: false,
        shift: false,
        msgs: vec![Msg { user: false, kind: "message".into(), text: "Hi, I am Pawline. Try: make a folder called kitten paws".into() }],
        confirm: None,
        focus: Btn::No,
        scroll: 0.0,
        plan_scroll: 0.0,
        busy: false,
        font,
        cache: HashMap::new(),
        tx: req_tx,
        exit: false,
        layer_mode,
        r_yes: (0.0, 0.0, 0.0, 0.0),
        r_no: (0.0, 0.0, 0.0, 0.0),
        r_collapse: (0.0, 0.0, 0.0, 0.0),
        ptr_pos: (0.0, 0.0),
        plan_max: 0.0,
        chat_max: 0.0,
    };
    if std::env::var("PAWLINE_START_EXPANDED").is_ok() {
        app.expanded = true;
        app.w = W;
        app.h = H;
        if let Surf::Layer(l) = &app.surf {
            l.set_size(W, H);
        }
    }

    let mut event_loop: EventLoop<App> = EventLoop::try_new().unwrap();
    let handle = event_loop.handle();
    WaylandSource::new(conn.clone(), queue).insert(handle.clone()).unwrap();
    handle
        .insert_source(back_rx, |ev, _, app: &mut App| {
            if let channel::Event::Msg(r) = ev {
                app.on_reply(r);
            }
        })
        .unwrap();
    let _ = calloop::timer::Timer::immediate();
    while !app.exit {
        event_loop.dispatch(None, &mut app).unwrap();
    }
}
