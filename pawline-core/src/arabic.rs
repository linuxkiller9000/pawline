//! Arabic commands (item 11a). A fixed set of Arabic request shapes is translated into the English command the normal parser
//! already understands. Only the fixed command words are matched, after folding spelling variants (diacritics, tatweel, alef and
//! ya/ta-marbuta forms, Arabic-Indic digits). Everything the user names (files, folders, places, topics) is passed through
//! byte for byte, never folded or corrected. Questions, negations and how-to phrases never run anything.
//! Scope limits (stated in docs/ARABIC.md): a fixed phrase set, not general Arabic understanding; command replies and confirm
//! cards stay English; no dialect coverage claim; typos are suggest-first on the command word only.

/// Fold spelling variants for MATCHING only.
pub fn fold(s: &str) -> String {
    s.chars()
        .filter_map(|c| match c {
            '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{0640}' => None,
            'أ' | 'إ' | 'آ' | 'ٱ' => Some('ا'),
            'ى' | 'ئ' => Some('ي'),
            'ة' => Some('ه'),
            'ؤ' => Some('و'),
            '٠'..='٩' => char::from_digit(c as u32 - '٠' as u32, 10),
            '۰'..='۹' => char::from_digit(c as u32 - '۰' as u32, 10),
            '؟' => Some('?'),
            '،' => Some(','),
            '؛' => Some(';'),
            _ => Some(c),
        })
        .collect()
}

static CHAT_N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn has_arabic(s: &str) -> bool {
    s.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c))
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Run this English command through the normal path, under exactly the rules that path already has (confirm card where it asks for one).
    Command(String),
    /// Static Arabic text (help, greeting). Nothing runs.
    Reply(String),
    /// A command word looks misspelled: offer this English command as a pick, never run it.
    Suggest { english: String, said: String, meant: String },
    Pass,
}

const CREATE: &[&str] = &["اعمل", "اعملي", "سوي", "اصنع", "انشي", "ضيف", "اضف", "كريت", "تعمل", "تنشي", "تسوي"];
const DELETE: &[&str] = &["احذف", "امسح", "ازل", "شيل", "ارمي", "احذفي", "امسحي", "تحذف", "تمسح"];
const RENAME: &[&str] = &["غير", "غيري", "عدل"];
const MOVE: &[&str] = &["انقل", "حرك", "تنقل"];
const COPY: &[&str] = &["انسخ", "تنسخ"];
const UNDO: &[&str] = &["تراجع", "ارجع", "ارجعي"];
const FOLDER: &[&str] = &["مجلدا", "مجلد", "المجلد", "فولدر", "الفولدر", "دليل", "الدليل"];
const FILE: &[&str] = &["ملفا", "ملف", "الملف", "فايل", "الفايل"];
const FILLER: &[&str] = &["لي", "ليا", "من", "فضلك", "لو", "سمحت", "الرجاء", "ارجوك", "بليز", "ممكن", "يا", "لوسمحت"];
const NAME_MARK: &[&str] = &["اسمه", "اسمها", "باسم", "اسم", "سمه", "بعنوان", "عنوانه"];
const WEATHER: &[&str] = &["طقس", "الطقس", "جو", "الجو", "حراره", "الحراره"];
const NOT_A_REQUEST: &[&str] = &["لا", "ما", "مش", "لن", "لم", "كيف", "ازاي", "هل", "ليه", "لماذا", "ماذا", "متي", "اذا", "ايش", "شو", "وش", "ليش", "علاش", "بدون"];
const HELP: &[&str] = &["مساعده", "المساعده", "ساعدني", "اوامر", "الاوامر", "help"];
const PLACES: &[(&str, &str)] = &[
    ("سطح المكتب", "Desktop"),
    ("المكتب", "Desktop"),
    ("المستندات", "Documents"),
    ("الوثائق", "Documents"),
    ("مستنداتي", "Documents"),
    ("التنزيلات", "Downloads"),
    ("التحميلات", "Downloads"),
    ("المنزل", "home"),
    ("الرئيسيه", "home"),
];

pub const HELP_AR: &str = "أقدر أفهم هذه الأوامر بالعربي: \n- اعمل مجلد اسمه مشاريع (أو: اعمل ملف اسمه notes.txt)، ويمكن إضافة: في المستندات / في سطح المكتب / في التنزيلات\n- احذف المجلد اسمه  أو احذف الملف اسمه\n- غير اسم a.txt الى b.txt\n- انقل a.txt الى المستندات، وانسخ a.txt الى التنزيلات\n- تراجع (للتراجع عن آخر عملية)\n- كيف الطقس في القاهرة؟\nكل أمر يغيّر ملفاتك يظهر لك أولاً في بطاقة تأكيد فيها المسار الكامل، ولا يحدث شيء قبل موافقتك. الأسماء التي تكتبها لا تُغيَّر أبداً. الردود وبطاقات التأكيد بالإنجليزية حالياً.\n(English help: say \"help\".)";

fn strip_quotes_ok(s: &str) -> String {
    s.trim().to_string()
}

fn place_of(folded_words: &[String]) -> Option<&'static str> {
    let phrase = folded_words.join(" ");
    PLACES.iter().find(|(a, _)| fold(a) == phrase).map(|(_, e)| *e)
}

/// Tokens: (original, folded).
type Tok = (String, String);

fn toks(input: &str) -> Vec<Tok> {
    input.split_whitespace().map(|t| (t.to_string(), fold(t))).collect()
}

fn join_orig(t: &[Tok]) -> String {
    t.iter().map(|x| x.0.as_str()).collect::<Vec<_>>().join(" ")
}

fn skip_filler(t: &[Tok]) -> &[Tok] {
    let mut i = 0;
    while i < t.len() && FILLER.contains(&t[i].1.as_str()) {
        i += 1;
    }
    &t[i..]
}

/// Split a trailing "في/داخل/جوه <known place>" off a name. Only a KNOWN place is split off, so a name like "صور في العيد" stays whole.
fn split_place(rest: &[Tok]) -> (&[Tok], Option<&'static str>) {
    for k in (0..rest.len()).rev() {
        if matches!(rest[k].1.as_str(), "في" | "داخل" | "جوه" | "جوا" | "علي") && k > 0 {
            let place: Vec<String> = rest[k + 1..].iter().map(|x| x.1.clone()).collect();
            if let Some(p) = place_of(&place) {
                return (&rest[..k], Some(p));
            }
        }
    }
    (rest, None)
}

fn kind_of(w: &str) -> Option<bool> {
    if FOLDER.contains(&w) {
        Some(true)
    } else if FILE.contains(&w) {
        Some(false)
    } else {
        None
    }
}

fn command_tail(rest: &[Tok]) -> (Option<bool>, &[Tok]) {
    let rest = skip_filler(rest);
    match rest.first().and_then(|t| kind_of(&t.1)) {
        Some(k) => (Some(k), &rest[1..]),
        None => (None, rest),
    }
}


const ADJ: &[&str] = &["جديدا", "جديد", "جديده", "فاضي", "فارغ", "فاضيه", "فارغه"];

/// Words before an explicit name marker may only be adjectives like "جديد" or one known "في <place>" phrase; anything else means
/// the request is not understood (fail closed), so a location or condition is never silently dropped.
fn split_marked(rest: &[Tok]) -> Option<(&[Tok], Option<&'static str>)> {
    let strong = |w: &str| matches!(w, "اسمه" | "اسمها" | "باسم" | "سمه" | "بعنوان" | "عنوانه");
    let Some(k) = rest.iter().position(|x| strong(&x.1)) else { return Some((rest, None)) };
    let mut pre: Vec<&Tok> = rest[..k].iter().filter(|x| !ADJ.contains(&x.1.as_str()) && !matches!(x.1.as_str(), "الذي" | "اللي" | "التي")).collect();
    let mut place = None;
    if !pre.is_empty() {
        if matches!(pre[0].1.as_str(), "في" | "داخل" | "جوه" | "جوا") {
            let words: Vec<String> = pre[1..].iter().map(|x| x.1.clone()).collect();
            place = Some(place_of(&words)?);
            pre.clear();
        }
    }
    if !pre.is_empty() {
        return None;
    }
    Some((&rest[k + 1..], place))
}

/// The English command for an Arabic imperative, or None. `verb_ok` lets the typo path ask the same question about a fixed head word.
fn command(t: &[Tok]) -> Option<String> {
    let t = skip_filler(t);
    let head = t.first()?.1.as_str();
    let rest = &t[1..];
    if CREATE.contains(&head) {
        let (kind, rest) = command_tail(rest);
        let kind = kind?;
        let rest = skip_filler(rest);
        let (rest, place_before) = split_marked(rest)?;
        let rest = match rest.first() {
            Some(x) if NAME_MARK.contains(&x.1.as_str()) => &rest[1..],
            _ => rest,
        };
        let (name, place_after) = split_place(rest);
        if name.is_empty() || (place_before.is_some() && place_after.is_some()) {
            return None;
        }
        let place = place_before.or(place_after);
        let name = strip_quotes_ok(&join_orig(name));
        let mut out = format!("make a {} called {}", if kind { "folder" } else { "file" }, name);
        if let Some(p) = place {
            out = format!("in {p} {out}");
        }
        return Some(out);
    }
    if DELETE.contains(&head) {
        let (kind, rest) = command_tail(rest);
        let rest = skip_filler(rest);
        let (rest, place_before) = split_marked(rest)?;
        let rest = match rest.first() {
            Some(x) if NAME_MARK.contains(&x.1.as_str()) => &rest[1..],
            _ => rest,
        };
        if rest.is_empty() || place_before.is_some() {
            return None;
        }
        let noun = match kind {
            Some(true) => "the folder ",
            Some(false) => "the file ",
            None => "",
        };
        return Some(format!("delete {noun}{}", join_orig(rest)));
    }
    if RENAME.contains(&head) && rest.first().is_some_and(|x| x.1 == "اسم") {
        let (_, rest) = command_tail(&rest[1..]);
        let k = rest.iter().position(|x| x.1 == "الي")?;
        let (a, b) = (&rest[..k], &rest[k + 1..]);
        if a.is_empty() || b.is_empty() {
            return None;
        }
        return Some(format!("rename {} to {}", join_orig(a), join_orig(b)));
    }
    if MOVE.contains(&head) || COPY.contains(&head) {
        let (_, rest) = command_tail(rest);
        let k = rest.iter().position(|x| x.1 == "الي")?;
        let (a, b) = (&rest[..k], &rest[k + 1..]);
        if a.is_empty() || b.is_empty() {
            return None;
        }
        let place: Vec<String> = b.iter().map(|x| x.1.clone()).collect();
        let dest = match place_of(&place) {
            Some(p) => p.to_string(),
            None => join_orig(b),
        };
        return Some(format!("{} {} to {}", if MOVE.contains(&head) { "move" } else { "copy" }, join_orig(a), dest));
    }
    if UNDO.contains(&head) && rest.len() <= 4 {
        return Some("undo".into());
    }
    None
}

fn weather(t: &[Tok]) -> Option<String> {
    let k = t.iter().position(|x| WEATHER.contains(&x.1.as_str()))?;
    if k > 3 {
        return None;
    }
    let mut rest = &t[k + 1..];
    if rest.first().is_some_and(|x| matches!(x.1.as_str(), "في" | "ب" | "عن" | "ل" | "لـ" | "بـ")) {
        rest = &rest[1..];
    }
    let mut when = "";
    let mut end = rest.len();
    if let Some(l) = rest.last() {
        let w = match l.1.as_str() {
            "اليوم" => " today",
            "الان" | "دلوقتي" | "حاليا" => " now",
            "بكره" | "غدا" | "بكرا" => " tomorrow",
            _ => "",
        };
        if !w.is_empty() && rest.len() > 1 {
            when = w;
            end -= 1;
        }
    }
    let place = join_orig(&rest[..end]);
    let place = place.trim_end_matches(|c: char| c == '?' || c == '؟' || c == '.' || c == '!').trim().to_string();
    if place.is_empty() {
        return None;
    }
    Some(format!("weather in {place}{when}"))
}

/// Presentation shapes (item 11c): "اعمل عرض تقديمي من notes.txt" and "اكتب عرض تقديمي عن دورة الماء".
/// The file name or topic is passed through byte for byte. The "about" form only reaches the normal path's online disclosure card.
fn deck(t: &[Tok]) -> Option<String> {
    let t = skip_filler(t);
    let head = t.first()?.1.as_str();
    if !(CREATE.contains(&head) || head == "اكتب") {
        return None;
    }
    let mut i = 1;
    if t.get(i).is_some_and(|x| x.1 == "لي") {
        i += 1;
    }
    let first = t.get(i)?.1.as_str();
    if first == "عرض" && t.get(i + 1).is_some_and(|x| x.1 == "تقديمي") {
        i += 2;
    } else if matches!(first, "بوربوينت" | "سلايدات" | "شرايح" | "بريزنتيشن" | "عرضا") {
        i += 1;
        if first == "عرضا" && t.get(i).is_some_and(|x| x.1 == "تقديميا") {
            i += 1;
        }
    } else {
        return None;
    }
    let marker = t.get(i)?.1.as_str();
    let rest = &t[i + 1..];
    if rest.is_empty() {
        return None;
    }
    let arg = join_orig(rest);
    let arg = arg.trim().to_string();
    match marker {
        "من" => Some(format!("make a presentation from {arg}")),
        "عن" | "حول" => Some(format!("write a presentation about {arg}")),
        _ => None,
    }
}

fn greeting(f: &str) -> Option<&'static str> {
    let f = f.trim().trim_end_matches(['!', '.', '?']).trim();
    match f {
        "السلام عليكم" | "سلام عليكم" | "السلام عليكم ورحمه الله" | "سلام" => Some("وعليكم السلام ورحمة الله وبركاته! اكتب \"مساعدة\" لأعرض لك ما أفهمه."),
        "مرحبا" | "اهلا" | "هلا" | "اهلا وسهلا" | "صباح الخير" | "مساء الخير" | "هاي" => Some("أهلاً بك! اكتب \"مساعدة\" لأعرض لك ما أفهمه."),
        "شكرا" | "شكرا لك" | "شكرا جزيلا" | "تسلم" | "يعطيك العافيه" => Some("العفو! أنا هنا متى احتجت."),
        _ => None,
    }
}

pub const JOKES_AR: &[&str] = &[
    "لماذا يحب المبرمجون الوضع المظلم؟ لأن الضوء يجذب الحشرات (bugs).",
    "قال المجلد للطبيب: أشعر أن بداخلي مجلدات فرعية كثيرة لم أحلّها.",
    "ما أحب أمر للقطط؟ sudo خرخر.",
    "سألت الحاسوب: تعبت، فقال لي: لا مشكلة، سأنام أنا.",
];

/// Small fixed chat set (item 11d). Whole-message matches only, deterministic, nothing runs, nothing is read or sent.
fn chat(f: &str, n: usize) -> Option<String> {
    let f = f.trim().trim_end_matches(['!', '.', '?', ',']).trim();
    let r = match f {
        "كيف حالك" | "كيف الحال" | "شلونك" | "ازيك" | "كيفك" | "عامل ايه" | "اخبارك" => "بخير، شكراً! جاهز لمساعدتك في الملفات والمجلدات. اكتب \"مساعدة\" لأعرض لك ما أفهمه.".to_string(),
        "من انت" | "مين انت" | "ما اسمك" | "شو اسمك" | "ايش اسمك" => "أنا Pawline، مساعد يعمل على جهازك. أفهم عدداً محدوداً من الأوامر بالعربي. اكتب \"مساعدة\" لتراها.".to_string(),
        "ماذا تستطيع" | "ماذا تفعل" | "ماذا تستطيع ان تفعل" | "ايش تقدر تسوي" | "شو بتعمل" | "ماذا يمكنك ان تفعل" => HELP_AR.to_string(),
        "قل نكته" | "احك نكته" | "احكي نكته" | "قول نكته" | "اعطني نكته" | "نكته" | "قل لي نكته" | "احكي لي نكته" | "ممكن نكته" => JOKES_AR[n % JOKES_AR.len()].to_string(),
        "نكته اخري" | "نكته تانيه" | "واحده اخري" | "كمان نكته" | "اخري" => JOKES_AR[n % JOKES_AR.len()].to_string(),
        _ => return None,
    };
    Some(r)
}

fn is_request_blocked(t: &[Tok], raw: &str) -> bool {
    raw.contains('؟') || raw.trim_end().ends_with('?') || t.first().is_some_and(|x| NOT_A_REQUEST.contains(&x.1.as_str()))
}

/// Translate one message. `Pass` means "not Arabic or not a fixed request shape": the normal path decides.
pub fn translate(input: &str) -> Outcome {
    // Only a request that STARTS in Arabic is translated. An English command that merely contains an Arabic name
    // ("make a folder called مشاريع") is already English and goes straight to the normal path.
    let t = toks(input);
    if !t.first().is_some_and(|x| has_arabic(&x.0)) {
        return Outcome::Pass;
    }
    let whole = fold(input);
    if let Some(g) = greeting(&whole) {
        return Outcome::Reply(g.into());
    }
    if let Some(c) = chat(&whole, CHAT_N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)) {
        return Outcome::Reply(c);
    }
    if t.len() <= 3 && HELP.contains(&whole.trim().trim_end_matches(['!', '.', '?']).trim()) {
        return Outcome::Reply(HELP_AR.into());
    }
    let blocked = is_request_blocked(&t, input);
    if !blocked {
        if let Some(c) = deck(&t).or_else(|| command(&t)) {
            return Outcome::Command(c);
        }
    }
    // Weather is read-only, so a question form is fine, but a negation never runs. A command shape was already tried first.
    if !t.first().is_some_and(|x| matches!(x.1.as_str(), "لا" | "مش" | "لن" | "لم")) {
        if let Some(w) = weather(&t) {
            return Outcome::Command(w);
        }
    }
    if blocked {
        return Outcome::Reply("هذا يبدو سؤالاً أو نفياً، فلم أنفّذ شيئاً. إن أردت أمراً اكتبه مباشرة، مثل: اعمل مجلد اسمه مشاريع. (Nothing was run: this reads as a question or a negation.)".into());
    }
    // Command-word typo: only the first word, only distance 1, only when the fixed head word is unique and the rest then parses.
    let head = skip_filler(&t).first().map(|x| x.1.clone());
    if let Some(h) = head {
        if h.chars().count() >= 4 {
            let all: Vec<&str> = CREATE.iter().chain(DELETE).chain(MOVE).chain(COPY).chain(RENAME).chain(UNDO).copied().collect();
            let near: Vec<&str> = all.iter().copied().filter(|v| crate::typo::distance(&h, v) == 1 && *v != h).collect();
            let mut uniq = near.clone();
            uniq.sort();
            uniq.dedup();
            if uniq.len() == 1 {
                let mut t2: Vec<Tok> = t.clone();
                let pos = t.iter().position(|x| x.1 == h).unwrap_or(0);
                t2[pos] = (uniq[0].to_string(), uniq[0].to_string());
                if let Some(c) = command(&t2) {
                    return Outcome::Suggest { english: c, said: t[pos].0.clone(), meant: uniq[0].to_string() };
                }
            }
        }
    }
    Outcome::Reply("لم أفهم هذا الطلب، ولم أنفّذ شيئاً. اكتب \"مساعدة\" لأعرض لك الأوامر التي أفهمها بالعربي. (I did not understand this, nothing was run. Say \"مساعدة\" for the Arabic commands I know.)".into())
}
