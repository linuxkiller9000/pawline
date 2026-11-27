//! Windows stand-ins for the key types the Wayland toolkit supplies on Linux. Values are X11 keysyms, the same numbers
//! the X11 backend sends, so the shared key handling in main.rs is the code Linux already runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Keysym(pub u32);

#[allow(non_upper_case_globals)]
impl Keysym {
    pub const BackSpace: Keysym = Keysym(0xff08);
    pub const Tab: Keysym = Keysym(0xff09);
    pub const Return: Keysym = Keysym(0xff0d);
    pub const Escape: Keysym = Keysym(0xff1b);
    pub const Home: Keysym = Keysym(0xff50);
    pub const Left: Keysym = Keysym(0xff51);
    pub const Up: Keysym = Keysym(0xff52);
    pub const Right: Keysym = Keysym(0xff53);
    pub const Down: Keysym = Keysym(0xff54);
    pub const Page_Up: Keysym = Keysym(0xff55);
    pub const Page_Down: Keysym = Keysym(0xff56);
    pub const End: Keysym = Keysym(0xff57);
    pub const Insert: Keysym = Keysym(0xff63);
    pub const KP_Enter: Keysym = Keysym(0xff8d);
    pub const F2: Keysym = Keysym(0xffbf);
    pub const Delete: Keysym = Keysym(0xffff);
    pub const space: Keysym = Keysym(0x20);
    pub const comma: Keysym = Keysym(0x2c);
    pub const A: Keysym = Keysym(b'A' as u32);
    pub const C: Keysym = Keysym(b'C' as u32);
    pub const R: Keysym = Keysym(b'R' as u32);
    pub const V: Keysym = Keysym(b'V' as u32);
    pub const X: Keysym = Keysym(b'X' as u32);
    pub const a: Keysym = Keysym(b'a' as u32);
    pub const c: Keysym = Keysym(b'c' as u32);
    pub const r: Keysym = Keysym(b'r' as u32);
    pub const v: Keysym = Keysym(b'v' as u32);
    pub const x: Keysym = Keysym(b'x' as u32);
    /// Only used for key events that carry no text on Windows (typed text arrives separately).
    #[allow(dead_code)]
    pub fn key_char(&self) -> Option<char> {
        if (0x20..0x7f).contains(&self.0) { char::from_u32(self.0) } else { None }
    }
    pub fn new(raw: u32) -> Keysym {
        Keysym(raw)
    }
}

pub struct KeyEvent {
    pub time: u32,
    pub raw_code: u32,
    pub keysym: Keysym,
    pub utf8: Option<String>,
}
