//! Deterministic math answers. The model never guesses arithmetic: a query
//! that is math is evaluated here, by a plain recursive-descent parser, and
//! returned as a side-effect-free Answer operation.

const MAX_LEN: usize = 200;

/// If the input is a math question, return the evaluated answer text.
/// Returns None for anything that is not clearly math, so the normal
/// parser/assist paths keep owning it.
pub fn try_answer(input: &str) -> Option<String> {
    let t = input.trim();
    if t.len() > MAX_LEN {
        return None;
    }
    let lower = t.to_lowercase();

    // "20% of 85" style
    if let Some(ans) = percent_of(&lower) {
        return Some(ans);
    }

    let expr = strip_trigger(&lower)?;
    let expr = expr.trim();
    if expr.is_empty() || !expr.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    if !expr
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | '+' | '-' | '*' | '/' | '^' | '%' | '(' | ')' | ' ' | 'x'))
    {
        return None;
    }
    let expr = expr.replace('x', "*");
    let mut p = Parser {
        chars: expr.as_bytes(),
        pos: 0,
        div_zero: false,
    };
    let v = match p.expr() {
        Ok(v) => v,
        Err(()) if p.div_zero => {
            // It IS math; the model must never guess it. Say so plainly.
            return Some(format!("{} is undefined (division by zero)", expr_display(expr.trim())));
        }
        Err(()) => return None,
    };
    p.skip_ws();
    if p.pos != p.chars.len() {
        return None; // trailing junk: not a clean expression
    }
    Some(format!("{} = {}", expr_display(expr.trim()), fmt_num(v)))
}

fn strip_trigger(lower: &str) -> Option<&str> {
    for pre in [
        "what is", "what's", "whats", "calculate", "compute", "how much is", "solve", "eval",
    ] {
        if let Some(rest) = lower.strip_prefix(pre) {
            return Some(rest.trim_end_matches(['?', ' ']).trim());
        }
    }
    // bare expression, optionally ending in ? or = or = ?
    let core = lower.trim_end_matches(['?', ' ']).trim_end_matches('=').trim();
    if core.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '(' || c == '-') {
        return Some(core);
    }
    None
}

fn percent_of(lower: &str) -> Option<String> {
    let (mut pct, rest) = lower.split_once("% of ")?;
    // tolerate a question prefix on the percent part: "what is 20% of 85"
    pct = pct.trim();
    if let Some(idx) = pct.rfind(' ') {
        pct = pct[idx + 1..].trim();
    }
    let pct: f64 = pct.parse().ok()?;
    let rest = rest.trim_end_matches(['?', ' ']).trim();
    if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    let base: f64 = rest.parse().ok()?;
    Some(format!(
        "{}% of {} = {}",
        fmt_num(pct),
        fmt_num(base),
        fmt_num(pct / 100.0 * base)
    ))
}

fn fmt_num(v: f64) -> String {
    if v.is_nan() || v.is_infinite() {
        return "undefined".into();
    }
    let rounded = (v * 1e6).round() / 1e6;
    if rounded == rounded.trunc() && rounded.abs() < 1e15 {
        format!("{}", rounded as i64)
    } else {
        let s = format!("{rounded:.6}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn expr_display(e: &str) -> String {
    e.split_whitespace().collect::<Vec<_>>().join(" ")
}

struct Parser<'a> {
    chars: &'a [u8],
    pos: usize,
    div_zero: bool,
}

impl Parser<'_> {
    fn skip_ws(&mut self) {
        while self.pos < self.chars.len() && self.chars[self.pos] == b' ' {
            self.pos += 1;
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.chars.get(self.pos).copied()
    }
    fn eat(&mut self, b: u8) -> bool {
        if self.peek() == Some(b) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expr(&mut self) -> Result<f64, ()> {
        let mut v = self.term()?;
        loop {
            if self.eat(b'+') {
                v += self.term()?;
            } else if self.eat(b'-') {
                v -= self.term()?;
            } else {
                return Ok(v);
            }
        }
    }
    fn term(&mut self) -> Result<f64, ()> {
        let mut v = self.unary()?;
        loop {
            if self.eat(b'*') {
                v *= self.unary()?;
            } else if self.eat(b'/') {
                let d = self.unary()?;
                if d == 0.0 {
                    self.div_zero = true;
                    return Err(());
                }
                v /= d;
            } else {
                return Ok(v);
            }
        }
    }
    fn unary(&mut self) -> Result<f64, ()> {
        if self.eat(b'-') {
            Ok(-self.unary()?)
        } else {
            self.pow()
        }
    }
    fn pow(&mut self) -> Result<f64, ()> {
        let base = self.primary()?;
        if self.eat(b'^') {
            let e = self.unary()?; // right-associative
            if e.abs() > 10_000.0 {
                return Err(());
            }
            Ok(base.powf(e))
        } else {
            Ok(base)
        }
    }
    fn primary(&mut self) -> Result<f64, ()> {
        if self.eat(b'(') {
            let v = self.expr()?;
            if !self.eat(b')') {
                return Err(());
            }
            return self.postfix_pct(v);
        }
        self.skip_ws();
        let start = self.pos;
        let mut seen_dot = false;
        while self.pos < self.chars.len() {
            let c = self.chars[self.pos];
            if c.is_ascii_digit() {
                self.pos += 1;
            } else if c == b'.' && !seen_dot {
                seen_dot = true;
                self.pos += 1;
            } else {
                break;
            }
        }
        if start == self.pos {
            return Err(());
        }
        let text = std::str::from_utf8(&self.chars[start..self.pos]).map_err(|_| ())?;
        let v: f64 = text.parse().map_err(|_| ())?;
        self.postfix_pct(v)
    }
    fn postfix_pct(&mut self, v: f64) -> Result<f64, ()> {
        if self.eat(b'%') {
            Ok(v / 100.0)
        } else {
            Ok(v)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::try_answer as a;

    #[test]
    fn basic() {
        assert_eq!(a("what is 2 + 2").as_deref(), Some("2 + 2 = 4"));
        assert_eq!(a("calculate 2+3*4").as_deref(), Some("2+3*4 = 14"));
        assert_eq!(a("what's (2+3) * 4?").as_deref(), Some("(2+3) * 4 = 20"));
    }

    #[test]
    fn division_and_power() {
        assert_eq!(a("7 / 2").as_deref(), Some("7 / 2 = 3.5"));
        assert_eq!(a("what is 2^10").as_deref(), Some("2^10 = 1024"));
        assert_eq!(a("compute 10/4").as_deref(), Some("10/4 = 2.5"));
    }

    #[test]
    fn percent() {
        assert_eq!(a("what is 20% of 85?").as_deref(), Some("20% of 85 = 17"));
        assert_eq!(a("50%").as_deref(), Some("50% = 0.5"));
    }

    #[test]
    fn negatives_and_decimals() {
        assert_eq!(a("-5 + 3").as_deref(), Some("-5 + 3 = -2"));
        assert_eq!(a("0.1 + 0.2").as_deref(), Some("0.1 + 0.2 = 0.3"));
    }

    #[test]
    fn not_math() {
        assert_eq!(a("make a folder called kitten paws"), None);
        assert_eq!(a("delete the folder called math"), None);
        assert_eq!(a("what is the weather"), None);
        assert_eq!(a("how much is my folder worth"), None);
    }

    #[test]
    fn wired_through_parse() {
        use crate::intent::parse;
        use crate::plan::Directive;
        assert_eq!(
            parse("what is 2 + 2").unwrap(),
            vec![Directive::Answer {
                text: "2 + 2 = 4".into()
            }]
        );
        assert_eq!(
            parse("what is 20% of 85?").unwrap(),
            vec![Directive::Answer {
                text: "20% of 85 = 17".into()
            }]
        );
        assert!(parse("make a folder called kitten paws").is_ok());
        assert!(parse("what is the weather").is_err());
    }

    #[test]
    fn bad_math() {
        assert_eq!(
            a("what is 1/0").as_deref(),
            Some("1/0 is undefined (division by zero)")
        ); // never model-guessed
        assert_eq!(a("what is 2 +"), None);
    }
}
