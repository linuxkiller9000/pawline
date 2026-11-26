//! Item 11a. The tuned list is regression only; the held-out list and the negative controls were written after the code by its
//! author, so they are not independent evidence. They are counted and reported as they come out.
use pawline_core::arabic::{fold, translate, Outcome};
use pawline_core::intent;

fn cmd(s: &str) -> String {
    match translate(s) {
        Outcome::Command(c) => c,
        o => panic!("{s}: expected a command, got {o:?}"),
    }
}

const TUNED: &[(&str, &str)] = &[
    ("اعمل مجلد اسمه مشاريع", "make a folder called مشاريع"),
    ("أنشئ ملف باسم notes.txt في المستندات", "in Documents make a file called notes.txt"),
    ("اِعْمَلْ مُجَلَّد اسمه x", "make a folder called x"),
    ("اعمـــل مجلد test في سطح المكتب", "in Desktop make a folder called test"),
    ("ممكن تحذف الملف a.txt", "delete the file a.txt"),
    ("احذف المجلد اسمه صور في العيد", "delete the folder صور في العيد"),
    ("غير اسم a.txt الى b.txt", "rename a.txt to b.txt"),
    ("غيّر اسم الملف a.txt إلى b.txt", "rename a.txt to b.txt"),
    ("انقل a.txt الى سطح المكتب", "move a.txt to Desktop"),
    ("انسخ a.txt إلى التنزيلات", "copy a.txt to Downloads"),
    ("تراجع", "undo"),
    ("كيف الطقس في القاهرة؟", "weather in القاهرة"),
    ("الطقس في القاهرة اليوم", "weather in القاهرة today"),
    ("طقس الاسكندرية بكره", "weather in الاسكندرية tomorrow"),
    ("اعمل مجلد اسمه ٢٠٢٦", "make a folder called ٢٠٢٦"),
    ("اعمل مجلد جو في المستندات", "in Documents make a folder called جو"),
];

const HELD_OUT: &[(&str, &str)] = &[
    ("لو سمحت اعمل لي مجلد اسمه صور", "make a folder called صور"),
    ("سوي ملف اسمه todo.md", "make a file called todo.md"),
    ("امسح الملف old.txt", "delete the file old.txt"),
    ("شيل المجلد القديم", "delete the folder القديم"),
    ("انقل report.pdf الى المستندات", "move report.pdf to Documents"),
    ("حرك a.txt الى التحميلات", "move a.txt to Downloads"),
    ("ارجع", "undo"),
    ("ايش الجو في الرياض", "weather in الرياض"),
    ("أنشي فولدر باسم أعمال", "make a folder called أعمال"),
    ("عدل اسم x.txt الى y.txt", "rename x.txt to y.txt"),
    ("الجو في دبي الان", "weather in دبي now"),
    ("ضيف ملف اسمه a.py داخل التنزيلات", "in Downloads make a file called a.py"),
];

const HARDER: &[(&str, &str)] = &[
    ("اعمل لي فولدر جديد اسمه المشاريع", "make a folder called المشاريع"),
    ("اعمل مجلد جديد في سطح المكتب باسم عمل", "in Desktop make a folder called عمل"),
    ("أنشئ مجلدا جديدا اسمه خطط", "make a folder called خطط"),
];

const FAIL_CLOSED: &[&str] = &[
    "احذف الملف القديم اللي اسمه old.txt",
    "اعمل مجلد في مكان غريب باسم x",
    "اعمل مجلد كبير جدا اسمه x",
    "احذف الملف في سطح المكتب اسمه x",
    "اعمل مجلد في المستندات باسم x في التنزيلات",
];

const NOT_REQUESTS: &[&str] = &[
    "لا تحذف الملف a.txt",
    "كيف احذف ملف؟",
    "هل تقدر تحذف مجلد",
    "ماذا يعني احذف",
    "ما تمسح الملف",
    "مش عايز احذف المجلد",
    "اعمل لي قهوة",
    "انا احب المجلدات",
    "المجلد الكبير",
    "الملف ده مهم جدا",
    "حذف الملفات خطر",
    "سأحذف الملف غدا",
    "ما هو الطقس؟",
    "لا الطقس مش حلو",
    "احذف",
    "اعمل مجلد",
    "ليش تراجع",
    "لماذا امسح الملف؟",
    "غيّر اسم الملف",
    "انقل a.txt",
];

#[test]
fn tuned_phrases_translate_exactly_and_parse() {
    for (a, e) in TUNED {
        let c = cmd(a);
        assert_eq!(&c, e, "{a}");
        if !c.starts_with("weather") {
            assert!(intent::parse(&c).is_ok(), "{c} must parse in the normal path: {:?}", intent::parse(&c));
        }
    }
}

#[test]
fn held_out_phrases_are_counted_not_assumed() {
    let mut ok = 0;
    let mut bad = vec![];
    for (a, e) in HELD_OUT {
        match translate(a) {
            Outcome::Command(c) if &c == e => ok += 1,
            o => bad.push(format!("{a} -> {o:?} (wanted {e})")),
        }
    }
    println!("held-out: {ok}/{} ; misses: {bad:#?}", HELD_OUT.len());
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn questions_negations_and_non_commands_never_run() {
    for s in NOT_REQUESTS {
        assert!(!matches!(translate(s), Outcome::Command(_)), "{s}: {:?}", translate(s));
    }
}

#[test]
fn payload_is_never_folded_or_corrected() {
    // diacritics, alef variants and digits inside a NAME survive byte for byte
    let c = cmd("اعمل مجلد اسمه أَحمد٣");
    assert_eq!(c, "make a folder called أَحمد٣");
    assert_eq!(fold("أَحمد٣"), "احمد3");
    // a place that is not a known place stays part of the name
    assert_eq!(cmd("اعمل مجلد اسمه صور في العيد"), "make a folder called صور في العيد");
}

#[test]
fn a_misspelled_command_word_is_only_suggested() {
    match translate("احزف الملف a.txt") {
        Outcome::Suggest { english, said, meant } => {
            assert_eq!(english, "delete the file a.txt");
            assert_eq!((said.as_str(), meant.as_str()), ("احزف", "احذف"));
        }
        o => panic!("{o:?}"),
    }
    // a name that is close to a verb is never touched: it is not the head word
    assert_eq!(cmd("احذف الملف احزف"), "delete the file احزف");
    // too far or ambiguous: no suggestion
    assert!(!matches!(translate("حزفوا الملف a.txt"), Outcome::Suggest { .. } | Outcome::Command(_)));
}

#[test]
fn help_and_greetings_are_static_arabic_text_and_run_nothing() {
    for s in ["مساعدة", "السلام عليكم", "مرحباً", "شكراً"] {
        assert!(matches!(translate(s), Outcome::Reply(_)), "{s}");
    }
    assert_eq!(translate("hello"), Outcome::Pass);
    assert_eq!(translate("make a folder called x"), Outcome::Pass);
    assert_eq!(translate("make a folder called مشاريع"), Outcome::Pass);
    assert_eq!(translate("weather in القاهرة"), Outcome::Pass);
}

#[test]
fn harder_phrases_and_fail_closed_cases() {
    let mut ok = 0;
    let mut bad = vec![];
    for (a, e) in HARDER {
        match translate(a) {
            Outcome::Command(c) if &c == e => ok += 1,
            o => bad.push(format!("{a} -> {o:?} (wanted {e})")),
        }
    }
    println!("harder: {ok}/{} ; misses: {bad:#?}", HARDER.len());
    for s in FAIL_CLOSED {
        assert!(!matches!(translate(s), Outcome::Command(_)), "{s}: a condition or place would be dropped: {:?}", translate(s));
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// Item 11c. Author-written after the code, not independent. Only the translation is checked here; the normal path owns the cards.
#[test]
fn presentation_shapes_translate_and_keep_payload_verbatim() {
    for (a, e) in [
        ("اعمل عرض تقديمي من notes.txt", "make a presentation from notes.txt"),
        ("أنشئ لي عرضاً تقديمياً من ملاحظاتي.txt", "make a presentation from ملاحظاتي.txt"),
        ("اكتب عرض تقديمي عن دورة الماء", "write a presentation about دورة الماء"),
        ("اعمل لي شرائح عن القطط", "write a presentation about القطط"),
    ] {
        assert_eq!(cmd(a), e, "{a}");
    }
    for bad in ["هل تعمل عرض تقديمي من a.txt؟", "لا تعمل عرض تقديمي من a.txt", "اعمل عرض تقديمي", "اعمل عرض تقديمي في a.txt", "كيف اعمل عرض تقديمي عن القطط"] {
        assert!(!matches!(translate(bad), Outcome::Command(_)), "{bad} must not run");
    }
}

/// Item 11d. Fixed Arabic chat answers: whole-message matches only. Author-written, not independent.
#[test]
fn small_arabic_chat_replies_and_never_runs_anything() {
    for a in ["كيف حالك؟", "من أنت", "قل لي نكتة", "نكتة أخرى", "ماذا تستطيع أن تفعل؟"] {
        match translate(a) {
            Outcome::Reply(t) => assert!(has(&t), "{a}"),
            o => panic!("{a}: {o:?}"),
        }
    }
    // Chat words inside a bigger request do not turn it into chat or a command.
    for a in ["كيف حالك احذف المجلد x", "قل نكتة واحذف ملف a.txt"] {
        assert!(!matches!(translate(a), Outcome::Command(_)), "{a}");
    }
}
fn has(s: &str) -> bool {
    pawline_core::arabic::has_arabic(s)
}
