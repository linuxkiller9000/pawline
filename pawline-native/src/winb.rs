//! Windows window, input and clipboard backend. UNVERIFIED: it cross-compiles, but nothing here has run on Windows.
//! It mirrors the X11 backend (same `Xb` API and the same `XEv` events, keysyms are X11 numbers), so the shared drawing
//! and key handling in main.rs is the code Linux already runs. The window is a layered, always-on-top popup that shows
//! the tiny-skia pixels with per-pixel alpha (UpdateLayeredWindow). A dedicated thread owns the window and its messages.
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use tiny_skia::Pixmap;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::DataExchange::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub enum XEv {
    Key { sym: u32, state: u16 },
    /// Typed text from WM_CHAR (any language). `sym` is the keysym when it is also a named key (space), else 0.
    Text { sym: u32, text: String },
    Press { button: u8, x: f64, y: f64, rx: f64, ry: f64, state: u16 },
    Release { x: f64, y: f64 },
    Motion { x: f64, y: f64, rx: f64, ry: f64, state: u16 },
    Expose,
}

static TX: OnceLock<Mutex<calloop::channel::Sender<XEv>>> = OnceLock::new();
static HIGH: AtomicIsize = AtomicIsize::new(0); // pending UTF-16 high surrogate

fn send(e: XEv) {
    if let Some(t) = TX.get() {
        if let Ok(g) = t.lock() {
            let _ = g.send(e);
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn mods() -> u16 {
    // SAFETY: GetKeyState has no preconditions.
    unsafe {
        let down = |vk: u16| (GetKeyState(vk as i32) as u16 & 0x8000) != 0;
        let mut m = 0;
        if down(VK_SHIFT) {
            m |= 1;
        }
        if down(VK_CONTROL) {
            m |= 4;
        }
        if down(VK_MENU) {
            m |= 8;
        }
        m
    }
}

fn keysym_of(vk: u16) -> Option<u32> {
    Some(match vk {
        VK_BACK => 0xff08,
        VK_TAB => 0xff09,
        VK_RETURN => 0xff0d,
        VK_ESCAPE => 0xff1b,
        VK_HOME => 0xff50,
        VK_LEFT => 0xff51,
        VK_UP => 0xff52,
        VK_RIGHT => 0xff53,
        VK_DOWN => 0xff54,
        VK_PRIOR => 0xff55,
        VK_NEXT => 0xff56,
        VK_END => 0xff57,
        VK_INSERT => 0xff63,
        VK_DELETE => 0xffff,
        _ => return None,
    })
}

fn pos(lp: LPARAM) -> (i32, i32) {
    (((lp & 0xffff) as u16 as i16) as i32, (((lp >> 16) & 0xffff) as u16 as i16) as i32)
}

fn root_of(hwnd: HWND, x: i32, y: i32) -> (f64, f64) {
    let mut p = POINT { x, y };
    // SAFETY: hwnd is our live window; p is a valid POINT.
    unsafe { ClientToScreen(hwnd, &mut p) };
    (p.x as f64, p.y as f64)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let vk = wp as u16;
            let m = mods();
            if let Some(sym) = keysym_of(vk) {
                send(XEv::Key { sym, state: m });
                return 0;
            }
            // Ctrl chords (copy, paste, ...) carry no text: send the lowercase letter as a key
            if m & 4 != 0 && m & 8 == 0 && (b'A' as u16..=b'Z' as u16).contains(&vk) {
                send(XEv::Key { sym: vk as u32 + 32, state: m });
                return 0;
            }
        }
        WM_CHAR => {
            let u = wp as u32;
            if u >= 0xD800 && u < 0xDC00 {
                HIGH.store(u as isize, Ordering::SeqCst);
                return 0;
            }
            let ch = if u >= 0xDC00 && u < 0xE000 {
                let hi = HIGH.swap(0, Ordering::SeqCst) as u32;
                if hi == 0 { None } else { char::from_u32(0x10000 + ((hi - 0xD800) << 10) + (u - 0xDC00)) }
            } else {
                char::from_u32(u)
            };
            if let Some(c) = ch {
                // control characters (Enter, Tab, Backspace, Ctrl+letter) arrive through WM_KEYDOWN instead
                if !c.is_control() && mods() & 4 == 0 {
                    send(XEv::Text { sym: if c == ' ' { 0x20 } else { 0 }, text: c.to_string() });
                }
            }
            return 0;
        }
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
            let (x, y) = pos(lp);
            let (rx, ry) = root_of(hwnd, x, y);
            let button = match msg {
                WM_LBUTTONDOWN => 1,
                WM_MBUTTONDOWN => 2,
                _ => 3,
            };
            SetCapture(hwnd);
            send(XEv::Press { button, x: x as f64, y: y as f64, rx, ry, state: mods() });
            return 0;
        }
        WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP => {
            let (x, y) = pos(lp);
            ReleaseCapture();
            send(XEv::Release { x: x as f64, y: y as f64 });
            return 0;
        }
        WM_MOUSEMOVE => {
            let (x, y) = pos(lp);
            let (rx, ry) = root_of(hwnd, x, y);
            send(XEv::Motion { x: x as f64, y: y as f64, rx, ry, state: mods() });
            return 0;
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp >> 16) & 0xffff) as u16 as i16;
            let (x, y) = pos(lp); // screen coordinates for the wheel
            let _ = (x, y);
            send(XEv::Press { button: if delta > 0 { 4 } else { 5 }, x: 0.0, y: 0.0, rx: 0.0, ry: 0.0, state: mods() });
            return 0;
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            BeginPaint(hwnd, &mut ps);
            EndPaint(hwnd, &ps);
            send(XEv::Expose);
            return 0;
        }
        WM_MOUSEACTIVATE => return MA_ACTIVATE as LRESULT,
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

pub struct Xb {
    hwnd: Arc<AtomicIsize>,
    shown: AtomicBool,
}

impl Xb {
    pub fn open(w: u16, h: u16, tx: calloop::channel::Sender<XEv>) -> Result<Xb, String> {
        let _ = TX.set(Mutex::new(tx));
        let (rtx, rrx) = mpsc::channel::<Result<isize, String>>();
        std::thread::spawn(move || unsafe {
            SetProcessDPIAware();
            let hinst = GetModuleHandleW(std::ptr::null());
            let cls = wide("PawlineWindow");
            let wc = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(wndproc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinst,
                hIcon: std::ptr::null_mut(),
                hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: cls.as_ptr(),
            };
            RegisterClassW(&wc);
            let title = wide("Pawline");
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                cls.as_ptr(),
                title.as_ptr(),
                WS_POPUP,
                0,
                0,
                w as i32,
                h as i32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                hinst,
                std::ptr::null(),
            );
            if hwnd.is_null() {
                let _ = rtx.send(Err(format!("CreateWindowExW failed ({})", GetLastError())));
                return;
            }
            let _ = rtx.send(Ok(hwnd as isize));
            let mut m: MSG = std::mem::zeroed();
            while GetMessageW(&mut m, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&m);
                DispatchMessageW(&m);
            }
        });
        let h = rrx.recv().map_err(|e| e.to_string())??;
        Ok(Xb { hwnd: Arc::new(AtomicIsize::new(h)), shown: AtomicBool::new(false) })
    }

    fn h(&self) -> HWND {
        self.hwnd.load(Ordering::SeqCst) as HWND
    }

    /// Work area (x, y, w, h) of the primary monitor: the screen minus the taskbar. Multi-monitor and live work-area
    /// changes are not handled.
    pub fn workarea(&self) -> (i32, i32, i32, i32) {
        let mut r = RECT { left: 0, top: 0, right: 1280, bottom: 720 };
        // SAFETY: r is a valid RECT for SPI_GETWORKAREA.
        unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut r as *mut RECT as *mut _, 0) };
        (r.left, r.top, r.right - r.left, r.bottom - r.top)
    }

    pub fn place(&self, x: i32, y: i32, w: u32, h: u32) {
        // SAFETY: our own live window.
        unsafe { SetWindowPos(self.h(), HWND_TOPMOST, x, y, w as i32, h as i32, SWP_NOACTIVATE) };
    }

    pub fn show(&mut self) {
        // SAFETY: our own live window.
        unsafe { ShowWindow(self.h(), SW_SHOWNOACTIVATE) };
        self.shown.store(true, Ordering::SeqCst);
    }

    pub fn hide(&mut self) {
        // SAFETY: our own live window.
        unsafe { ShowWindow(self.h(), SW_HIDE) };
        self.shown.store(false, Ordering::SeqCst);
    }

    pub fn is_shown(&self) -> bool {
        self.shown.load(Ordering::SeqCst)
    }

    pub fn focus(&self) {
        // SAFETY: our own live window. Windows may refuse to steal focus from another app; then a click focuses it.
        unsafe {
            SetForegroundWindow(self.h());
            SetFocus(self.h());
        }
    }

    pub fn present(&mut self, pm: &Pixmap) {
        let (w, h) = (pm.width() as i32, pm.height() as i32);
        // SAFETY: standard GDI sequence on handles created and released here; the DIB is w*h*4 bytes.
        unsafe {
            let screen = GetDC(std::ptr::null_mut());
            let mem = CreateCompatibleDC(screen);
            let mut bi: BITMAPINFO = std::mem::zeroed();
            bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bi.bmiHeader.biWidth = w;
            bi.bmiHeader.biHeight = -h;
            bi.bmiHeader.biPlanes = 1;
            bi.bmiHeader.biBitCount = 32;
            bi.bmiHeader.biCompression = BI_RGB;
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let bmp = CreateDIBSection(mem, &bi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
            if bmp.is_null() || bits.is_null() {
                DeleteDC(mem);
                ReleaseDC(std::ptr::null_mut(), screen);
                return;
            }
            let dst = std::slice::from_raw_parts_mut(bits as *mut u8, (w * h * 4) as usize);
            // tiny-skia: premultiplied RGBA. A layered window wants premultiplied BGRA.
            for (d, s) in dst.chunks_exact_mut(4).zip(pm.data().chunks_exact(4)) {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
                d[3] = s[3];
            }
            let old = SelectObject(mem, bmp);
            let size = SIZE { cx: w, cy: h };
            let src = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
            UpdateLayeredWindow(self.h(), screen, std::ptr::null(), &size, mem, &src, 0, &blend, ULW_ALPHA);
            SelectObject(mem, old);
            DeleteObject(bmp);
            DeleteDC(mem);
            ReleaseDC(std::ptr::null_mut(), screen);
        }
    }

    pub fn clip_copy(&self, text: String) -> bool {
        let u: Vec<u16> = wide(&text);
        // SAFETY: the global block is sized for the UTF-16 text plus NUL; ownership passes to the clipboard on success.
        unsafe {
            if OpenClipboard(self.h()) == 0 {
                return false;
            }
            EmptyClipboard();
            let mem = GlobalAlloc(GMEM_MOVEABLE, u.len() * 2);
            let mut ok = false;
            if !mem.is_null() {
                let p = GlobalLock(mem) as *mut u16;
                if !p.is_null() {
                    std::ptr::copy_nonoverlapping(u.as_ptr(), p, u.len());
                    GlobalUnlock(mem);
                    ok = !SetClipboardData(13 /* CF_UNICODETEXT */, mem as HANDLE).is_null();
                }
                if !ok {
                    GlobalFree(mem);
                }
            }
            CloseClipboard();
            ok
        }
    }

    pub fn clip_paste(&self) -> Option<String> {
        // SAFETY: the clipboard text is read while the clipboard is open and the block is locked.
        unsafe {
            if OpenClipboard(self.h()) == 0 {
                return None;
            }
            let mut out = None;
            let h = GetClipboardData(13);
            if !h.is_null() {
                let p = GlobalLock(h as _) as *const u16;
                if !p.is_null() {
                    let mut n = 0;
                    while *p.add(n) != 0 && n < 1_000_000 {
                        n += 1;
                    }
                    out = Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, n)));
                    GlobalUnlock(h as _);
                }
            }
            CloseClipboard();
            out
        }
    }
}
