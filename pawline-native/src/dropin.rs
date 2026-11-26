//! Shared helpers for paste and drag-and-drop payloads: turn text/uri-list
//! (what file managers offer for copied or dragged files) into plain paths,
//! and decode an offered blob into insertable text. Pure functions; the
//! Wayland and X11 front-ends both use them.

/// Percent-decode a URI component ("%20" -> ' '). Invalid sequences are kept
/// verbatim (never lose user data by guessing).
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 3 <= b.len() {
            let h = (b[i + 1] as char).to_digit(16);
            let l = (b[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (h, l) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// One URI from a uri-list to what the user means: file:// URIs become local
/// paths (file://localhost/... counts as local), anything else stays verbatim.
pub fn uri_to_path(uri: &str) -> String {
    let rest = match uri.strip_prefix("file://") {
        Some(r) => r,
        None => return uri.to_string(),
    };
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    percent_decode(path)
}

/// A whole text/uri-list payload (CRLF or LF separated, # comment lines) to
/// single-line text for the input box: paths/URIs joined by one space.
pub fn uri_list_to_paths(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(uri_to_path)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Decode an offered blob given its mime type. Uri-lists become paths; plain
/// text mimes pass through as lossy UTF-8 - but sources like xclip serve the
/// same bytes under any target, so text that is clearly a uri-list (a line
/// starts with file://) is converted too. Empty result means nothing usable.
pub fn offered_to_text(mime: &str, bytes: &[u8]) -> Option<String> {
    let raw = String::from_utf8_lossy(bytes).into_owned();
    let looks_like_uris = raw.lines().any(|l| l.trim_start().starts_with("file://"));
    let out = if mime == "text/uri-list" || looks_like_uris {
        uri_list_to_paths(&raw)
    } else {
        raw
    };
    let out = out.trim().to_string();
    if out.is_empty() { None } else { Some(out) }
}

/// Mime types Pawline accepts for paste and drop, best first: plain text,
/// then file-manager uri-lists (converted to paths by offered_to_text).
pub const PASTE_MIMES: [&str; 6] = [
    "text/plain;charset=utf-8",
    "text/plain",
    "text/uri-list",
    "UTF8_STRING",
    "TEXT",
    "STRING",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decode_basic_and_invalid() {
        assert_eq!(percent_decode("/home/a/My%20Pictures/8.jpg"), "/home/a/My Pictures/8.jpg");
        assert_eq!(percent_decode("/x/%zz%4"), "/x/%zz%4"); // invalid kept verbatim
        assert_eq!(percent_decode("/x/100%.jpg"), "/x/100%.jpg"); // trailing % kept
        assert_eq!(percent_decode("plain"), "plain");
    }

    #[test]
    fn file_uris_become_paths() {
        assert_eq!(uri_to_path("file:///home/a/8.jpg"), "/home/a/8.jpg");
        assert_eq!(uri_to_path("file://localhost/home/a/8.jpg"), "/home/a/8.jpg");
        assert_eq!(uri_to_path("file:///home/a/My%20Pictures/8.jpg"), "/home/a/My Pictures/8.jpg");
        assert_eq!(uri_to_path("https://example.com/x"), "https://example.com/x");
    }

    #[test]
    fn uri_list_joins_and_skips_comments() {
        let list = "# comment\r\nfile:///a/1.jpg\r\nfile:///b/2%20x.jpg\n\n";
        assert_eq!(uri_list_to_paths(list), "/a/1.jpg /b/2 x.jpg");
    }

    #[test]
    fn offered_text_by_mime() {
        assert_eq!(offered_to_text("text/uri-list", b"file:///a/1.jpg\n"), Some("/a/1.jpg".into()));
        assert_eq!(offered_to_text("text/plain", b"hello"), Some("hello".into()));
        assert_eq!(offered_to_text("text/plain", b"   "), None);
        assert_eq!(offered_to_text("text/uri-list", b"# only a comment\n"), None);
        // served under a plain target but clearly a file URI (xclip, some file managers)
        assert_eq!(offered_to_text("text/plain", b"file:///a/My%20Pics/1.jpg\n"), Some("/a/My Pics/1.jpg".into()));
        // ordinary text mentioning a URL is untouched
        assert_eq!(offered_to_text("text/plain", b"see https://example.com/x"), Some("see https://example.com/x".into()));
        assert_eq!(offered_to_text("text/plain", b"https://example.com/x"), Some("https://example.com/x".into()));
    }
}
