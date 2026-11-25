//! Single-line edit buffer for the prompt box: caret, selection, sanitised paste.
//! Pure state, no I/O, so it is unit-tested without a window.

pub const MAX_CHARS: usize = 2000;

#[derive(Default, Clone, Debug, PartialEq)]
pub struct Edit {
    chars: Vec<char>,
    caret: usize,
    anchor: Option<usize>,
}

/// Pasted or typed text becomes one line of printable characters. Newlines, tabs and other
/// control characters turn into a single space and never reach the submit path.
pub fn sanitize(s: &str) -> String {
    let mut out = String::new();
    let mut last_space = false;
    for c in s.chars() {
        let c = if c.is_control() { ' ' } else { c };
        if c == ' ' && last_space && s.chars().any(char::is_control) {
            continue;
        }
        last_space = c == ' ';
        out.push(c);
    }
    out
}

impl Edit {
    pub fn text(&self) -> String { self.chars.iter().collect() }
    pub fn is_blank(&self) -> bool { self.chars.iter().all(|c| c.is_whitespace()) }
    pub fn caret(&self) -> usize { self.caret }
    pub fn clear(&mut self) { self.chars.clear(); self.caret = 0; self.anchor = None; }
    pub fn set_caret(&mut self, i: usize, extend: bool) {
        self.mark(extend);
        self.caret = i.min(self.chars.len());
        self.fix_anchor();
    }
    fn mark(&mut self, extend: bool) {
        if extend { if self.anchor.is_none() { self.anchor = Some(self.caret); } } else { self.anchor = None; }
    }
    fn fix_anchor(&mut self) { if self.anchor == Some(self.caret) { self.anchor = None; } }
    pub fn sel(&self) -> Option<(usize, usize)> { self.range() }
    fn range(&self) -> Option<(usize, usize)> {
        self.anchor.map(|a| (a.min(self.caret), a.max(self.caret)))
    }
    pub fn selected(&self) -> Option<String> {
        self.range().map(|(a, b)| self.chars[a..b].iter().collect())
    }
    /// Text to copy: the selection, or everything when nothing is selected.
    pub fn copy_text(&self) -> String { self.selected().unwrap_or_else(|| self.text()) }
    fn delete_selection(&mut self) -> bool {
        if let Some((a, b)) = self.range() {
            self.chars.drain(a..b);
            self.caret = a;
            self.anchor = None;
            true
        } else { false }
    }
    pub fn insert(&mut self, s: &str) {
        let s = sanitize(s);
        self.delete_selection();
        for c in s.chars() {
            if self.chars.len() >= MAX_CHARS { break; }
            self.chars.insert(self.caret, c);
            self.caret += 1;
        }
    }
    pub fn backspace(&mut self) {
        if self.delete_selection() { return; }
        if self.caret > 0 { self.caret -= 1; self.chars.remove(self.caret); }
    }
    pub fn delete(&mut self) {
        if self.delete_selection() { return; }
        if self.caret < self.chars.len() { self.chars.remove(self.caret); }
    }
    pub fn cut(&mut self) -> String { let t = self.copy_text(); if self.anchor.is_some() { self.delete_selection(); } else { self.clear(); } t }
    pub fn left(&mut self, extend: bool) {
        if !extend { if let Some((a, _)) = self.range() { self.caret = a; self.anchor = None; return; } }
        self.mark(extend); self.caret = self.caret.saturating_sub(1); self.fix_anchor();
    }
    pub fn right(&mut self, extend: bool) {
        if !extend { if let Some((_, b)) = self.range() { self.caret = b; self.anchor = None; return; } }
        self.mark(extend); self.caret = (self.caret + 1).min(self.chars.len()); self.fix_anchor();
    }
    pub fn home(&mut self, extend: bool) { self.mark(extend); self.caret = 0; self.fix_anchor(); }
    pub fn end(&mut self, extend: bool) { self.mark(extend); self.caret = self.chars.len(); self.fix_anchor(); }
    pub fn word_left(&mut self, extend: bool) {
        self.mark(extend);
        let mut i = self.caret;
        while i > 0 && self.chars[i - 1].is_whitespace() { i -= 1; }
        while i > 0 && !self.chars[i - 1].is_whitespace() { i -= 1; }
        self.caret = i; self.fix_anchor();
    }
    pub fn word_right(&mut self, extend: bool) {
        self.mark(extend);
        let n = self.chars.len();
        let mut i = self.caret;
        while i < n && !self.chars[i].is_whitespace() { i += 1; }
        while i < n && self.chars[i].is_whitespace() { i += 1; }
        self.caret = i; self.fix_anchor();
    }
    pub fn select_all(&mut self) { self.anchor = Some(0); self.caret = self.chars.len(); self.fix_anchor(); }
    /// (text before caret, text after caret) for drawing.
    pub fn split(&self) -> (String, String) {
        (self.chars[..self.caret].iter().collect(), self.chars[self.caret..].iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn e(s: &str) -> Edit { let mut x = Edit::default(); x.insert(s); x }

    #[test]
    fn caret_moves_and_inserts_in_the_middle() {
        let mut x = e("helo");
        x.left(false); x.insert("l");
        assert_eq!(x.text(), "hello");
        x.home(false); x.insert(">");
        assert_eq!(x.text(), ">hello");
        x.end(false); x.backspace();
        assert_eq!(x.text(), ">hell");
        x.home(false); x.delete();
        assert_eq!(x.text(), "hell");
    }
    #[test]
    fn words_and_selection() {
        let mut x = e("open  the folder");
        x.word_left(false);
        assert_eq!(x.caret(), 10);
        x.word_left(false);
        assert_eq!(x.caret(), 6);
        x.word_right(true);
        assert_eq!(x.selected().as_deref(), Some("the "));
        x.insert("a ");
        assert_eq!(x.text(), "open  a folder");
        x.select_all();
        assert_eq!(x.copy_text(), "open  a folder");
        assert_eq!(x.cut(), "open  a folder");
        assert!(x.is_blank());
    }
    #[test]
    fn paste_never_carries_newlines_or_controls() {
        let mut x = Edit::default();
        x.insert("delete the folder x\ny\r\nenter\u{7}\t!");
        assert!(!x.text().chars().any(|c| c.is_control()));
        assert_eq!(x.text(), "delete the folder x y enter !");
    }
    #[test]
    fn paste_is_length_capped_and_replaces_selection() {
        let mut x = Edit::default();
        x.insert(&"a".repeat(MAX_CHARS + 50));
        assert_eq!(x.text().chars().count(), MAX_CHARS);
        let mut y = e("abc def");
        y.select_all(); y.insert("z");
        assert_eq!(y.text(), "z");
    }
    #[test]
    fn copy_without_selection_copies_everything() {
        let x = e("2+2");
        assert_eq!(x.copy_text(), "2+2");
    }
}
