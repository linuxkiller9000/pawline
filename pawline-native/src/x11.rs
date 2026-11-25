//! X11 window backend (Xlib loaded at run time with dlopen via x11-dl, so the executable has no
//! link-time X11 dependency and still starts on a Wayland-only machine). Draws the same pixmap as
//! the Wayland path. Selected only in a plain local X11 session (see `main`).
use super::*;
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_long, c_uint, c_ulong};
use std::os::fd::{AsFd, BorrowedFd, RawFd};
use x11_dl::xlib;

/// Allowance for a window manager frame when keeping the window on screen.
const FRAME_L: c_int = 6;
const FRAME_R: c_int = 6;
const FRAME_TOP: c_int = 34;
const FRAME_B: c_int = 6;

struct Conn {
    x: xlib::Xlib,
    dpy: *mut xlib::Display,
    win: c_ulong,
    gc: xlib::GC,
    visual: *mut xlib::Visual,
    depth: c_int,
}

struct X11Out {
    c: std::rc::Rc<Conn>,
    buf: Vec<u8>,
}

impl Out for X11Out {
    fn present(&mut self, w: u32, h: u32, pm: &Pixmap) {
        let c = &self.c;
        self.buf.resize((w * h * 4) as usize, 0);
        let argb = c.depth == 32;
        for (d, s) in self.buf.chunks_exact_mut(4).zip(pm.data().chunks_exact(4)) {
            if argb {
                // tiny-skia premultiplied RGBA -> premultiplied BGRA (0xAARRGGBB little endian)
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
                d[3] = s[3];
            } else {
                // no alpha visual: composite over the card background colour
                let inv = 255 - s[3] as u32;
                d[0] = (s[2] as u32 + 40 * inv / 255).min(255) as u8;
                d[1] = (s[1] as u32 + 28 * inv / 255).min(255) as u8;
                d[2] = (s[0] as u32 + 30 * inv / 255).min(255) as u8;
                d[3] = 255;
            }
        }
        unsafe {
            let img = (c.x.XCreateImage)(c.dpy, c.visual, c.depth as c_uint, xlib::ZPixmap, 0, self.buf.as_mut_ptr() as *mut c_char, w, h, 32, 0);
            if img.is_null() {
                return;
            }
            (c.x.XPutImage)(c.dpy, c.win, c.gc, img, 0, 0, 0, 0, w, h);
            (*img).data = std::ptr::null_mut(); // the Vec owns the pixels
            (c.x.XDestroyImage)(img);
            (c.x.XFlush)(c.dpy);
        }
    }
    fn resize(&mut self, w: u32, h: u32) {
        let c = &self.c;
        unsafe {
            set_fixed_size(c, w, h);
            // Keep the right edge where it is and make sure the whole window (plus a WM frame
            // allowance) stays on the screen, so the expanded card's buttons are never off-screen.
            let screen = (c.x.XDefaultScreen)(c.dpy);
            let (sw, sh) = ((c.x.XDisplayWidth)(c.dpy, screen), (c.x.XDisplayHeight)(c.dpy, screen));
            let root = (c.x.XRootWindow)(c.dpy, screen);
            let (mut rx, mut ry, mut ch): (c_int, c_int, c_ulong) = (0, 0, 0);
            let mut wa: xlib::XWindowAttributes = std::mem::zeroed();
            (c.x.XGetWindowAttributes)(c.dpy, c.win, &mut wa);
            (c.x.XTranslateCoordinates)(c.dpy, c.win, root, 0, 0, &mut rx, &mut ry, &mut ch);
            let (ow, _oh) = (wa.width, wa.height);
            let right = rx + ow;
            let nx = (right - w as c_int).clamp(FRAME_L, (sw - w as c_int - FRAME_R).max(FRAME_L));
            let ny = ry.clamp(FRAME_TOP, (sh - h as c_int - FRAME_B).max(FRAME_TOP));
            (c.x.XMoveResizeWindow)(c.dpy, c.win, nx, ny, w, h);
            (c.x.XFlush)(c.dpy);
        }
    }
}

unsafe fn set_fixed_size(c: &Conn, w: u32, h: u32) {
    let hints = (c.x.XAllocSizeHints)();
    if hints.is_null() {
        return;
    }
    (*hints).flags = xlib::PMinSize | xlib::PMaxSize;
    (*hints).min_width = w as c_int;
    (*hints).max_width = w as c_int;
    (*hints).min_height = h as c_int;
    (*hints).max_height = h as c_int;
    (c.x.XSetWMNormalHints)(c.dpy, c.win, hints);
    (c.x.XFree)(hints as *mut _);
}

struct Fd(RawFd);
impl AsFd for Fd {
    fn as_fd(&self) -> BorrowedFd<'_> {
        // The connection outlives the event loop (both live in `run`).
        unsafe { BorrowedFd::borrow_raw(self.0) }
    }
}

unsafe fn atom(c: &Conn, name: &str) -> c_ulong {
    let n = CString::new(name).unwrap();
    (c.x.XInternAtom)(c.dpy, n.as_ptr(), 0)
}

/// Why the X11 window could not start (the caller falls back to terminal mode).
pub fn run(req_tx: mpsc::Sender<Req>, back_rx: channel::Channel<UiReply>) -> Result<(), String> {
    let x = xlib::Xlib::open().map_err(|e| format!("libX11 could not be loaded ({e})"))?;
    unsafe {
        let dpy = (x.XOpenDisplay)(std::ptr::null());
        if dpy.is_null() {
            return Err("could not connect to the X display".into());
        }
        let screen = (x.XDefaultScreen)(dpy);
        let root = (x.XRootWindow)(dpy, screen);
        // Prefer a 32-bit ARGB visual (rounded corners show through under a compositor).
        let mut vi: xlib::XVisualInfo = std::mem::zeroed();
        let have32 = (x.XMatchVisualInfo)(dpy, screen, 32, xlib::TrueColor, &mut vi) != 0;
        let (visual, depth, cmap) = if have32 {
            (vi.visual, 32, (x.XCreateColormap)(dpy, root, vi.visual, xlib::AllocNone))
        } else {
            ((x.XDefaultVisual)(dpy, screen), (x.XDefaultDepth)(dpy, screen), (x.XDefaultColormap)(dpy, screen))
        };
        let mut attrs: xlib::XSetWindowAttributes = std::mem::zeroed();
        attrs.colormap = cmap;
        attrs.border_pixel = 0;
        attrs.background_pixel = 0;
        attrs.event_mask = xlib::KeyPressMask
            | xlib::KeyReleaseMask
            | xlib::ButtonPressMask
            | xlib::PointerMotionMask
            | xlib::FocusChangeMask
            | xlib::ExposureMask
            | xlib::StructureNotifyMask;
        let win = (x.XCreateWindow)(
            dpy,
            root,
            (x.XDisplayWidth)(dpy, screen) - COMPACT as c_int - 20,
            FRAME_TOP + 6,
            COMPACT,
            COMPACT,
            0,
            depth,
            xlib::InputOutput as c_uint,
            visual,
            xlib::CWColormap | xlib::CWBorderPixel | xlib::CWBackPixel | xlib::CWEventMask,
            &mut attrs,
        );
        let gc = (x.XCreateGC)(dpy, win, 0, std::ptr::null_mut());
        let c = std::rc::Rc::new(Conn { x, dpy, win, gc, visual, depth });

        let title = CString::new("Pawline").unwrap();
        (c.x.XStoreName)(dpy, win, title.as_ptr());
        let mut ch = xlib::XClassHint { res_name: title.as_ptr() as *mut c_char, res_class: title.as_ptr() as *mut c_char };
        (c.x.XSetClassHint)(dpy, win, &mut ch);
        // Ask window managers (i3, openbox, ...) to float it like a small dialog/utility window.
        let wtype = atom(&c, "_NET_WM_WINDOW_TYPE");
        let dialog = atom(&c, "_NET_WM_WINDOW_TYPE_DIALOG");
        (c.x.XChangeProperty)(dpy, win, wtype, xlib::XA_ATOM, 32, xlib::PropModeReplace, &dialog as *const c_ulong as *const u8, 1);
        let mut del = atom(&c, "WM_DELETE_WINDOW");
        (c.x.XSetWMProtocols)(dpy, win, &mut del, 1);
        set_fixed_size(&c, COMPACT, COMPACT);

        let ui = Ui::new(Box::new(X11Out { c: c.clone(), buf: Vec::new() }), req_tx);
        (c.x.XMapWindow)(dpy, win);
        (c.x.XFlush)(dpy);

        let mut event_loop: EventLoop<Ui> = EventLoop::try_new().map_err(|e| e.to_string())?;
        let handle = event_loop.handle();
        let fd = (c.x.XConnectionNumber)(dpy);
        let cc = c.clone();
        handle
            .insert_source(calloop::generic::Generic::new(Fd(fd), calloop::Interest::READ, calloop::Mode::Level), move |_, _, ui: &mut Ui| {
                drain(&cc, ui, del);
                Ok(calloop::PostAction::Continue)
            })
            .map_err(|e| e.to_string())?;
        handle
            .insert_source(back_rx, |ev, _, ui: &mut Ui| {
                if let channel::Event::Msg(r) = ev {
                    ui.on_reply(r);
                }
            })
            .map_err(|e| e.to_string())?;
        let mut ui = ui;
        ui.draw();
        while !ui.exit {
            event_loop.dispatch(None, &mut ui).map_err(|e| e.to_string())?;
        }
        ui.cancel_voice("");
        (c.x.XDestroyWindow)(dpy, win);
        (c.x.XCloseDisplay)(dpy);
    }
    Ok(())
}

unsafe fn drain(c: &Conn, ui: &mut Ui, del: c_ulong) {
    while (c.x.XPending)(c.dpy) > 0 {
        let mut ev: xlib::XEvent = std::mem::zeroed();
        (c.x.XNextEvent)(c.dpy, &mut ev);
        if std::env::var_os("PAWLINE_X11_TRACE").is_some() {
            eprintln!("x11 event type {}", ev.get_type());
        }
        match ev.get_type() {
            xlib::Expose => ui.draw(),
            xlib::FocusOut => {
                // Ignore grab-related focus changes (NotifyGrab/NotifyUngrab); a real loss closes the mic.
                if ev.focus_change.mode == xlib::NotifyNormal {
                    if std::env::var_os("PAWLINE_X11_TRACE").is_some() {
                        eprintln!("x11 focus lost -> focus_lost()");
                    }
                    ui.focus_lost();
                }
            }
            xlib::ClientMessage => {
                if ev.client_message.data.get_long(0) as c_ulong == del {
                    ui.cancel_voice("");
                    ui.exit = true;
                }
            }
            xlib::MotionNotify => ui.ptr_pos = (ev.motion.x as f64, ev.motion.y as f64),
            xlib::ButtonPress => {
                ui.ptr_pos = (ev.button.x as f64, ev.button.y as f64);
                match ev.button.button {
                    1 => ui.click(),
                    4 => ui.wheel(-40.0),
                    5 => ui.wheel(40.0),
                    _ => {}
                }
            }
            xlib::KeyPress => {
                let (ks, text) = lookup(c, &mut ev.key);
                ui.key(Ke { keysym: ks, utf8: text });
            }
            xlib::KeyRelease => {
                // X11 key repeat sends Release+Press pairs with the same time and keycode.
                // Hold-to-talk must see one continuous hold, so swallow both.
                if (c.x.XPending)(c.dpy) > 0 {
                    let mut nx: xlib::XEvent = std::mem::zeroed();
                    (c.x.XPeekEvent)(c.dpy, &mut nx);
                    if nx.get_type() == xlib::KeyPress && nx.key.keycode == ev.key.keycode && nx.key.time == ev.key.time {
                        (c.x.XNextEvent)(c.dpy, &mut nx);
                        let (ks, text) = lookup(c, &mut nx.key);
                        // a repeat of any key other than a held F9 is delivered as a normal press
                        if ks != Keysym::F9 {
                            ui.key(Ke { keysym: ks, utf8: text });
                        }
                        continue;
                    }
                }
                let (ks, _) = lookup(c, &mut ev.key);
                if ks == Keysym::F9 {
                    ui.voice_release();
                    ui.draw();
                }
            }
            _ => {}
        }
    }
}

unsafe fn lookup(c: &Conn, k: &mut xlib::XKeyEvent) -> (Keysym, Option<String>) {
    let mut ks: c_ulong = 0;
    let mut buf = [0 as c_char; 16];
    (c.x.XLookupString)(k, buf.as_mut_ptr(), buf.len() as c_int, &mut ks, std::ptr::null_mut());
    let keysym = Keysym::new(ks as u32);
    let ctrl = k.state & xlib::ControlMask != 0;
    let text = if ctrl { None } else { keysym.key_char().filter(|ch| !ch.is_control()).map(|ch| ch.to_string()) };
    let _ = c_long::MAX;
    (keysym, text)
}
