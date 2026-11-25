//! X11 window, input and clipboard for Pawline (XFCE and other X11 sessions).
//! The drawing is the same tiny-skia code as on Wayland. Only the window, the
//! keyboard/mouse events and the clipboard are different here.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use smithay_client_toolkit::reexports::calloop;
use tiny_skia::Pixmap;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::xproto::*;
use x11rb::protocol::{shape, Event};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME};

pub enum XEv {
    Key { sym: u32, state: u16 },
    Press { button: u8, x: f64, y: f64, rx: f64, ry: f64, state: u16 },
    Release { x: f64, y: f64 },
    Motion { x: f64, y: f64, rx: f64, ry: f64, state: u16 },
    Expose,
}

struct Atoms {
    clipboard: Atom,
    utf8: Atom,
    targets: Atom,
    text: Atom,
    prop: Atom,
    incr: Atom,
}

struct Shared {
    clip: Mutex<String>,
    owned: AtomicBool,
    paste: Mutex<Option<mpsc::Sender<Option<Vec<u8>>>>>,
}

pub struct Xb {
    conn: Arc<RustConnection>,
    win: Window,
    gc: Gcontext,
    root: Window,
    pub sw: u16,
    pub sh: u16,
    depth: u8,
    has_shape: bool,
    mapped: bool,
    atoms: Arc<Atoms>,
    shared: Arc<Shared>,
}

const CLIP_MAX: usize = 64 * 1024;

impl Xb {
    /// Open the X display and make the (unmapped) window. Err = no usable X11 display.
    pub fn open(w: u16, h: u16, tx: calloop::channel::Sender<XEv>) -> Result<Xb, String> {
        let (conn, scr) = RustConnection::connect(None).map_err(|e| format!("{e}"))?;
        let conn = Arc::new(conn);
        let setup = conn.setup();
        let screen = &setup.roots[scr];
        if !(screen.root_depth == 24 || screen.root_depth == 32) {
            return Err(format!("unsupported colour depth {}", screen.root_depth));
        }
        let win = conn.generate_id().map_err(|e| e.to_string())?;
        let ev = EventMask::EXPOSURE | EventMask::KEY_PRESS | EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION;
        // No window manager border, no taskbar entry: the orb is an overlay like the Wayland layer surface.
        conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            win,
            screen.root,
            0,
            0,
            w,
            h,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new().override_redirect(1).event_mask(ev).background_pixmap(0u32),
        )
        .map_err(|e| e.to_string())?;
        let gc = conn.generate_id().map_err(|e| e.to_string())?;
        conn.create_gc(gc, win, &CreateGCAux::new()).map_err(|e| e.to_string())?;
        conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_NAME, AtomEnum::STRING, b"Pawline").map_err(|e| e.to_string())?;
        conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_CLASS, AtomEnum::STRING, b"pawline\0Pawline\0").map_err(|e| e.to_string())?;
        let ia = |n: &[u8]| -> Result<Atom, String> { Ok(conn.intern_atom(false, n).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?.atom) };
        let atoms = Arc::new(Atoms {
            clipboard: ia(b"CLIPBOARD")?,
            utf8: ia(b"UTF8_STRING")?,
            targets: ia(b"TARGETS")?,
            text: ia(b"TEXT")?,
            prop: ia(b"PAWLINE_SEL")?,
            incr: ia(b"INCR")?,
        });
        let has_shape = std::env::var_os("PAWLINE_NO_SHAPE").is_none() && conn.extension_information(shape::X11_EXTENSION_NAME).ok().flatten().is_some();
        let shared = Arc::new(Shared { clip: Mutex::new(String::new()), owned: AtomicBool::new(false), paste: Mutex::new(None) });
        // keyboard map for keycode -> keysym
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let km = conn.get_keyboard_mapping(min, max - min + 1).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
        let per = km.keysyms_per_keycode as usize;
        let syms = km.keysyms;
        let (c2, a2, s2, sh2) = (conn.clone(), atoms.clone(), shared.clone(), win);
        std::thread::spawn(move || {
            while let Ok(ev) = c2.wait_for_event() {
                match ev {
                    Event::KeyPress(k) => {
                        let st = u16::from(k.state);
                        let i = (k.detail.saturating_sub(min)) as usize * per;
                        let col = (if st & 1 != 0 { 1 } else { 0 }) + if st & 0x80 != 0 && per >= 4 { 2 } else { 0 };
                        let mut sym = syms.get(i + col).copied().unwrap_or(0);
                        if sym == 0 {
                            sym = syms.get(i).copied().unwrap_or(0);
                        }
                        // Caps Lock: capitalise plain letters
                        if st & 2 != 0 && st & 1 == 0 && (0x61..=0x7a).contains(&sym) {
                            sym -= 0x20;
                        }
                        if sym != 0 && tx.send(XEv::Key { sym, state: st }).is_err() {
                            break;
                        }
                    }
                    Event::ButtonPress(b) => {
                        let _ = tx.send(XEv::Press { button: b.detail, x: b.event_x as f64, y: b.event_y as f64, rx: b.root_x as f64, ry: b.root_y as f64, state: u16::from(b.state) });
                    }
                    Event::ButtonRelease(b) => {
                        let _ = tx.send(XEv::Release { x: b.event_x as f64, y: b.event_y as f64 });
                    }
                    Event::MotionNotify(m) => {
                        let _ = tx.send(XEv::Motion { x: m.event_x as f64, y: m.event_y as f64, rx: m.root_x as f64, ry: m.root_y as f64, state: u16::from(m.state) });
                    }
                    Event::Expose(e) if e.count == 0 => {
                        let _ = tx.send(XEv::Expose);
                    }
                    Event::SelectionRequest(r) => {
                        let mut prop = r.property;
                        if r.target == a2.targets {
                            let _ = c2.change_property32(PropMode::REPLACE, r.requestor, r.property, AtomEnum::ATOM, &[a2.targets, a2.utf8, u32::from(AtomEnum::STRING), a2.text]);
                        } else if s2.owned.load(Ordering::SeqCst) && (r.target == a2.utf8 || r.target == a2.text || r.target == u32::from(AtomEnum::STRING)) {
                            let t = s2.clip.lock().map(|g| g.clone()).unwrap_or_default();
                            let ty = if r.target == a2.text { a2.utf8 } else { r.target };
                            let _ = c2.change_property8(PropMode::REPLACE, r.requestor, r.property, ty, t.as_bytes());
                        } else {
                            prop = 0;
                        }
                        let n = SelectionNotifyEvent { response_type: SELECTION_NOTIFY_EVENT, sequence: 0, time: r.time, requestor: r.requestor, selection: r.selection, target: r.target, property: prop };
                        let _ = c2.send_event(false, r.requestor, EventMask::NO_EVENT, n);
                        let _ = c2.flush();
                    }
                    Event::Error(e) => {
                        if std::env::var_os("PAWLINE_DEBUG").is_some() {
                            eprintln!("x11 error: {e:?}");
                        }
                    }
                    Event::SelectionClear(_) => s2.owned.store(false, Ordering::SeqCst),
                    Event::SelectionNotify(n) => {
                        let got = if n.property != 0 {
                            match c2.get_property(true, sh2, n.property, AtomEnum::ANY, 0, (CLIP_MAX / 4) as u32).ok().and_then(|c| c.reply().ok()) {
                                Some(r) if r.type_ != a2.incr => Some(r.value),
                                _ => None,
                            }
                        } else {
                            None
                        };
                        if let Ok(mut g) = s2.paste.lock() {
                            if let Some(t) = g.take() {
                                let _ = t.send(got);
                            }
                        }
                    }
                    _ => {}
                }
            }
        });
        Ok(Xb {
            conn: conn.clone(),
            win,
            gc,
            root: screen.root,
            sw: screen.width_in_pixels,
            sh: screen.height_in_pixels,
            depth: screen.root_depth,
            has_shape,
            mapped: false,
            atoms,
            shared,
        })
    }

    pub fn place(&self, x: i32, y: i32, w: u32, h: u32) {
        let _ = self.conn.configure_window(self.win, &ConfigureWindowAux::new().x(x).y(y).width(w).height(h));
        let _ = self.conn.flush();
    }

    pub fn show(&mut self) {
        if !self.mapped {
            let _ = self.conn.map_window(self.win);
            self.mapped = true;
        }
        let _ = self.conn.configure_window(self.win, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE));
        let _ = self.conn.flush();
    }

    pub fn hide(&mut self) {
        if self.mapped {
            let _ = self.conn.unmap_window(self.win);
            self.mapped = false;
            let _ = self.conn.flush();
        }
    }

    pub fn is_shown(&self) -> bool {
        self.mapped
    }

    /// Keyboard focus to the window (we are an override-redirect overlay, so the window manager won't do it).
    pub fn focus(&self) {
        if self.mapped {
            let _ = self.conn.set_input_focus(InputFocus::POINTER_ROOT, self.win, CURRENT_TIME);
            let _ = self.conn.flush();
        }
    }

    /// Draw a frame: pixels plus a window shape cut from the alpha channel (rounded corners without a compositor).
    pub fn present(&mut self, pm: &Pixmap) {
        let (w, h) = (pm.width() as u16, pm.height() as u16);
        let mut data = Vec::with_capacity(pm.data().len());
        for s in pm.data().chunks_exact(4) {
            // premultiplied RGBA -> BGRX; the shape hides transparent pixels, the rest is over black
            data.extend_from_slice(&[s[2], s[1], s[0], 0]);
        }
        if std::env::var_os("PAWLINE_DEBUG").is_some() { eprintln!("present {}x{} nonzero={} depth={} maxreq={}", w, h, data.iter().filter(|b| **b != 0).count(), self.depth, self.conn.maximum_request_bytes()); }
        if self.has_shape {
            // window shape = the pixels that are not transparent (rounded corners without a compositor)
            let (wu, hu) = (w as usize, h as usize);
            let mut rects: Vec<Rectangle> = Vec::new();
            let mut prev: Vec<(usize, usize)> = Vec::new();
            let mut open: Vec<(usize, usize, usize)> = Vec::new(); // x0, x1, y_start
            for y in 0..=hu {
                let mut runs: Vec<(usize, usize)> = Vec::new();
                if y < hu {
                    let mut x = 0;
                    while x < wu {
                        if pm.data()[(y * wu + x) * 4 + 3] >= 128 {
                            let x0 = x;
                            while x < wu && pm.data()[(y * wu + x) * 4 + 3] >= 128 {
                                x += 1;
                            }
                            runs.push((x0, x));
                        } else {
                            x += 1;
                        }
                    }
                }
                // close rectangles whose run did not continue, open new ones
                let mut still = Vec::new();
                for (x0, x1, ys) in open.drain(..) {
                    if runs.contains(&(x0, x1)) {
                        still.push((x0, x1, ys));
                    } else {
                        rects.push(Rectangle { x: x0 as i16, y: ys as i16, width: (x1 - x0) as u16, height: (y - ys) as u16 });
                    }
                }
                for r in &runs {
                    if !still.iter().any(|(a, b, _)| (*a, *b) == *r) {
                        still.push((r.0, r.1, y));
                    }
                }
                open = still;
                prev = runs;
            }
            let _ = prev;
            let _ = shape::rectangles(&*self.conn, shape::SO::SET, shape::SK::BOUNDING, ClipOrdering::UNSORTED, self.win, 0, 0, &rects);
            if std::env::var_os("PAWLINE_DEBUG").is_some() {
                eprintln!("shape rects sent={}", rects.len());
            }
        }
        // send in bands of rows so one request never exceeds the server's maximum request size
        let max = self.conn.maximum_request_bytes().saturating_sub(64).max(4096);
        let rows = (max / (w as usize * 4)).clamp(1, h as usize);
        let mut y = 0usize;
        while y < h as usize {
            let n = rows.min(h as usize - y);
            let part = &data[y * w as usize * 4..(y + n) * w as usize * 4];
            let _ = self.conn.put_image(ImageFormat::Z_PIXMAP, self.win, self.gc, w, n as u16, 0, y as i16, 0, self.depth, part);
            y += n;
        }
        let _ = self.conn.flush();
    }

    pub fn clip_copy(&self, text: String) -> bool {
        if let Ok(mut g) = self.shared.clip.lock() {
            *g = text;
        }
        self.shared.owned.store(true, Ordering::SeqCst);
        let _ = self.conn.set_selection_owner(self.win, self.atoms.clipboard, CURRENT_TIME);
        let _ = self.conn.flush();
        self.conn.get_selection_owner(self.atoms.clipboard).ok().and_then(|c| c.reply().ok()).is_some_and(|r| r.owner == self.win)
    }

    /// Read the clipboard as text; waits at most half a second, text only, 64 KB at most.
    pub fn clip_paste(&self) -> Option<String> {
        if self.shared.owned.load(Ordering::SeqCst) {
            return self.shared.clip.lock().ok().map(|g| g.clone());
        }
        let (tx, rx) = mpsc::channel();
        *self.shared.paste.lock().ok()? = Some(tx);
        let _ = self.conn.convert_selection(self.win, self.atoms.clipboard, self.atoms.utf8, self.atoms.prop, CURRENT_TIME);
        let _ = self.conn.flush();
        let got = rx.recv_timeout(std::time::Duration::from_millis(500)).ok().flatten();
        if let Ok(mut g) = self.shared.paste.lock() {
            *g = None;
        }
        got.map(|b| String::from_utf8_lossy(&b[..b.len().min(CLIP_MAX)]).into_owned())
    }

    #[allow(dead_code)]
    pub fn root(&self) -> Window {
        self.root
    }
}
