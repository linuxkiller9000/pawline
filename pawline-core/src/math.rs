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

    // bare "x = 19 - 8 * (4)": x alone on the left, a number expression on the right
    if let Some((l, r)) = lower.split_once('=') {
        let r = r.trim().trim_end_matches('?').trim();
        if l.trim() == "x" && r.chars().any(|c| c.is_ascii_digit()) && !r.contains('=') && !r.contains('x') {
            if let Some(ans) = solve_linear(&format!("solve {lower}")) {
                return Some(ans);
            }
        }
    }

    // linear equations in x: "solve 2x + 4 = 10"
    if lower.starts_with("solve") {
        if let Some(ans) = solve_linear(&lower) {
            return Some(ans);
        }
        // an equation shape we do not solve deterministically: say so
        // plainly instead of letting the model guess algebra
        if lower.contains('=') && lower.contains('x') {
            return Some(
                "I can only solve linear equations in x so far (like \"solve 2x + 4 = 10\")."
                    .into(),
            );
        }
    }

    // "20% of 85" style
    if let Some(ans) = percent_of(&lower) {
        return Some(ans);
    }

    let expr = strip_trigger(&lower)?;
    let expr = expr.trim();
    if expr.is_empty() || !expr.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    // An equation in x after any trigger ("what is 88+50-90 = x * (8)^2", "calculate 2x = 8"):
    // it IS math - solve it offline or refuse plainly; it never reaches the online model.
    if expr.contains('=') && expr.contains('x') {
        return Some(match solve_linear(&format!("solve {expr}")) {
            Some(ans) => ans,
            None => "I can only solve linear equations in x so far (like \"solve 2x + 4 = 10\")."
                .into(),
        });
    }
    // An expression in x with no equals sign is algebra, not arithmetic: say so, never guess.
    if expr.contains('x') {
        return Some(
            "That has an x but no equals sign, so there is nothing to solve. Give me an equation, like \"2x + 4 = 10\"."
                .into(),
        );
    }
    if !expr
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | '+' | '-' | '*' | '/' | '^' | '%' | '(' | ')' | ' '))
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

/// Deterministic solver for linear equations in one variable:
/// "solve 2x + 4 = 10" -> "2x + 4 = 10  =>  x = 3". Only linear terms
/// (ax + b = cx + d); anything more complex is refused so the model never
/// guesses algebra either.
fn solve_linear(lower: &str) -> Option<String> {
    let body = lower
        .strip_prefix("solve for x:")
        .or_else(|| lower.strip_prefix("solve for x,"))
        .or_else(|| lower.strip_prefix("solve for x"))
        .or_else(|| lower.strip_prefix("solve:"))
        .or_else(|| lower.strip_prefix("solve"))?
        .trim()
        .trim_end_matches(['?', ' '])
        .trim();
    let (lhs, rhs) = body.split_once('=')?;
    if rhs.contains('=') || !body.contains('x') {
        return None;
    }
    let (a1, b1) = linear_side(lhs)?;
    let (a2, b2) = linear_side(rhs)?;
    let eq = format!("{} = {}", lhs.trim(), rhs.trim());
    let (a, b) = (a1 - a2, b2 - b1);
    if a == 0.0 {
        return Some(if b == 0.0 {
            format!("{eq}  =>  true for every x (infinitely many solutions)")
        } else {
            format!("{eq}  =>  no solution")
        });
    }
    Some(format!("{eq}  =>  x = {}", fmt_num(b / a)))
}

/// Parse one side of a linear equation into (x_coefficient, constant) with the full expression
/// grammar: parentheses, *, /, ^ and % all work as long as the side stays linear in x after
/// simplification ("x * (8)^2" -> 64x; "(x + 2) * 3 - 4" -> 3x + 2). x^2 and x*x are refused.
fn linear_side(s: &str) -> Option<(f64, f64)> {
    let s = s.replace(' ', "");
    if s.is_empty()
        || !s
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | '+' | '-' | '*' | '/' | '^' | '%' | '(' | ')' | 'x'))
    {
        return None;
    }
    let mut p = AParser { chars: s.as_bytes(), pos: 0, bad: false };
    let v = p.expr().ok()?;
    if p.pos == p.chars.len() && !p.bad { Some(v) } else { None }
}

/// Affine value: a*x + b.
struct AParser<'a> {
    chars: &'a [u8],
    pos: usize,
    bad: bool,
}

impl AParser<'_> {
    fn peek(&mut self) -> Option<u8> {
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
    fn mul((a1, b1): (f64, f64), (a2, b2): (f64, f64), bad: &mut bool) -> (f64, f64) {
        if a1 == 0.0 {
            (b1 * a2, b1 * b2)
        } else if a2 == 0.0 {
            (a1 * b2, b1 * b2)
        } else {
            *bad = true; // x * x is not linear
            (0.0, 0.0)
        }
    }
    fn expr(&mut self) -> Result<(f64, f64), ()> {
        let mut v = self.term()?;
        loop {
            if self.eat(b'+') {
                let (a, b) = self.term()?;
                v = (v.0 + a, v.1 + b);
            } else if self.eat(b'-') {
                let (a, b) = self.term()?;
                v = (v.0 - a, v.1 - b);
            } else {
                return Ok(v);
            }
        }
    }
    fn term(&mut self) -> Result<(f64, f64), ()> {
        let mut v = self.unary()?;
        loop {
            if self.eat(b'*') {
                let u = self.unary()?;
                v = Self::mul(v, u, &mut self.bad);
            } else if self.eat(b'/') {
                let (a, b) = self.unary()?;
                if a != 0.0 {
                    self.bad = true; // dividing by an x-term is not linear
                    return Err(());
                }
                if b == 0.0 {
                    return Err(());
                }
                v = (v.0 / b, v.1 / b);
            } else if matches!(self.peek(), Some(b'x') | Some(b'(')) {
                // implicit multiplication: "2x", "2(x+1)", "(x+1)(x-1)"
                let u = self.unary()?;
                v = Self::mul(v, u, &mut self.bad);
            } else {
                return Ok(v);
            }
        }
    }
    fn unary(&mut self) -> Result<(f64, f64), ()> {
        if self.eat(b'-') {
            let (a, b) = self.unary()?;
            Ok((-a, -b))
        } else {
            self.pow()
        }
    }
    fn pow(&mut self) -> Result<(f64, f64), ()> {
        let base = self.primary()?;
        if self.eat(b'^') {
            let (ea, eb) = self.unary()?; // right-associative
            if ea != 0.0 || eb.abs() > 10_000.0 {
                self.bad = true; // x in the exponent, or absurd
                return Err(());
            }
            if eb == 0.0 {
                return Ok((0.0, 1.0));
            }
            if eb == 1.0 {
                return Ok(base);
            }
            if base.0 != 0.0 {
                self.bad = true; // x^2, x^0.5, ... not linear
                return Err(());
            }
            Ok((0.0, base.1.powf(eb)))
        } else {
            Ok(base)
        }
    }
    fn primary(&mut self) -> Result<(f64, f64), ()> {
        if self.eat(b'(') {
            let v = self.expr()?;
            if !self.eat(b')') {
                return Err(());
            }
            return self.postfix_pct(v);
        }
        if self.eat(b'x') {
            return Ok((1.0, 0.0));
        }
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
        self.postfix_pct((0.0, v))
    }
    fn postfix_pct(&mut self, (a, b): (f64, f64)) -> Result<(f64, f64), ()> {
        if self.eat(b'%') {
            Ok((a / 100.0, b / 100.0))
        } else {
            Ok((a, b))
        }
    }
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
    fn equations_linear_after_simplification() {
        // his case: 88+50-90 = 48 and x*(8)^2 = 64x, so x = 0.75
        assert_eq!(
            a("what is 88+50 - 90 = x * (8)^2").as_deref(),
            Some("88+50 - 90 = x * (8)^2  =>  x = 0.75")
        );
        assert_eq!(
            a("solve 88+50 - 90 = x * (8)^2").as_deref(),
            Some("88+50 - 90 = x * (8)^2  =>  x = 0.75")
        );
        assert_eq!(a("solve 2x + 4 = 10").as_deref(), Some("2x + 4 = 10  =>  x = 3"));
        assert_eq!(a("solve (x + 2) * 3 - 4 = 10").as_deref(), Some("(x + 2) * 3 - 4 = 10  =>  x = 2.666667"));
        assert_eq!(a("solve x / 4 + 1 = 3").as_deref(), Some("x / 4 + 1 = 3  =>  x = 8"));
        assert_eq!(a("solve 2(x + 1) = x + 5").as_deref(), Some("2(x + 1) = x + 5  =>  x = 3"));
        assert_eq!(a("solve 3x = x * 2 ^ 2").as_deref(), Some("3x = x * 2 ^ 2  =>  x = 0"));
    }

    #[test]
    fn equations_we_cannot_solve_are_plain_refusals_never_online() {
        // quadratic or x in the denominator: a clear offline answer, never None (no model fallback)
        for q in ["solve x^2 = 4", "solve x * x = 9", "solve 1 / x = 2", "what is x ^ 2 + 1 = 10"] {
            let r = a(q).expect(q);
            assert!(r.contains("linear"), "{q}: {r}");
        }
        // an expression in x with no equals sign: explained, not sent to the model
        let r = a("what is 2x + 3").expect("x expression");
        assert!(r.contains("no equals sign"), "{r}");
    }

    #[test]
    fn not_math() {
        assert_eq!(a("make a folder called kitten paws"), None);
        assert_eq!(a("delete the folder called math"), None);
        assert_eq!(a("what is the weather"), None);
        assert_eq!(a("how much is my folder worth"), None);
    }

    #[test]
    fn solve_linear_equations() {
        assert_eq!(a("solve 2x + 4 = 10").as_deref(), Some("2x + 4 = 10  =>  x = 3"));
        assert_eq!(a("solve for x: 3x - 9 = 0").as_deref(), Some("3x - 9 = 0  =>  x = 3"));
        assert_eq!(a("solve x + 5 = 2x - 3").as_deref(), Some("x + 5 = 2x - 3  =>  x = 8"));
        assert_eq!(a("solve 2x = 2x").as_deref(), Some("2x = 2x  =>  true for every x (infinitely many solutions)"));
        assert_eq!(a("solve 2x + 1 = 2x + 5").as_deref(), Some("2x + 1 = 2x + 5  =>  no solution"));
        // not linear: plain refusal, never model-guessed
        assert_eq!(
            a("solve x^2 = 4").as_deref(),
            Some("I can only solve linear equations in x so far (like \"solve 2x + 4 = 10\").")
        );
    }

    #[test]
    fn user_reported_solve_for_x_equals_expression() {
        assert_eq!(
            a("solve for x, x=19 - 8 * (4)").as_deref(),
            Some("x = 19 - 8 * (4)  =>  x = -13")
        );
        assert_eq!(a("solve x = 2 * (3 + 4)").as_deref(), Some("x = 2 * (3 + 4)  =>  x = 14"));
        assert_eq!(a("solve 2x = 3 * (4)").as_deref(), Some("2x = 3 * (4)  =>  x = 6"));
    }

    #[test]
    fn bare_x_equals_number_expression() {
        assert_eq!(a("x=19-8*(4)").as_deref(), Some("x = 19-8*(4)  =>  x = -13"));
        assert_eq!(a("x = 2 * (3 + 4)").as_deref(), Some("x = 2 * (3 + 4)  =>  x = 14"));
        assert!(a("x = y").is_none());
        assert!(a("x = x + 1").is_none());
        assert!(a("x=").is_none());
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
        assert!(matches!(parse("what is the weather").unwrap()[..], [crate::plan::Directive::Answer { .. }]), "asks which place instead of guessing");
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
