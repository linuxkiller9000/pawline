//! Held-out chat typo set, written after chat.rs and not used to tune it. Misses are printed, not hidden.
//! Hard asserts: no negative control triggers a joke/hint/small-talk reply, and no typo'd negation or how-to ever becomes a pick.
use pawline_core::chat::{ChatState, Outcome};

fn kind(s: &str) -> &'static str {
    let mut c = ChatState::default();
    match c.respond(s, false) {
        Outcome::Pass => "pass",
        Outcome::Reply(t) if t.starts_with("Hint:") => "hint",
        Outcome::Reply(t) if pawline_core::chat::JOKES.contains(&t.as_str()) => "joke",
        Outcome::Reply(_) => "reply",
        Outcome::Pick { .. } => "pick",
        Outcome::Discussion(_) => "discussion",
    }
}

const POS: &[(&str, &str)] = &[
    ("tell me a jkoe", "joke"), ("got any jokes?", "joke"), ("make me laugh", "joke"), ("say somthing funny", "joke"), ("tel me a joke please", "joke"),
    ("i need a hnit", "hint"), ("any tips?", "hint"), ("what can you do", "hint"), ("give me an idea", "hint"), ("hlep me", "hint"),
    ("hows it going", "reply"), ("how are yuo", "reply"), ("whos there, who are yuo", "reply"), ("thx", "reply"), ("thnaks a lot", "reply"),
    ("dont delte the folder kp", "discussion"), ("do not remvoe my files", "discussion"), ("never move that file", "discussion"),
    ("how do i delte a folder", "hint"), ("how can i reanme a file", "hint"),
    ("can you delte the folder kp", "pick"), ("could you creaet a folder called pics", "pick"),
];

const NEG: &[&str] = &[
    "I think clouds look like sheep", "the jokes on you", "that was a thin joke book", "tank is full", "a hint of lemon in the tea", "my friend tells jokes",
    "turn left at the light", "meet me at noon", "the weather is nice", "what is gravity", "write a poem about a cat", "the film was funny",
    "i like thanks giving", "hunting for a house", "how much is a tip at a restaurant",
];

#[test]
fn heldout_report() {
    let mut hit = 0;
    let mut misses = vec![];
    for (s, want) in POS {
        let k = kind(s);
        if k == *want { hit += 1 } else { misses.push(format!("{s} -> {k} (wanted {want})")) }
    }
    eprintln!("CHAT HELD-OUT positives: {hit}/{}; misses: {misses:?}", POS.len());
    let mut bad = vec![];
    for s in NEG {
        let k = kind(s);
        if matches!(k, "joke" | "hint" | "reply" | "pick") { bad.push(format!("{s} -> {k}")) }
    }
    eprintln!("CHAT HELD-OUT negatives wrongly triggered: {bad:?} of {}", NEG.len());
    // safety: a negated or how-to line is never a pick
    for (s, want) in POS {
        if matches!(*want, "discussion" | "hint") && s.contains("delte") || s.contains("reanme") || s.contains("remvoe") {
            assert!(kind(s) != "pick", "{s} must not be a pick");
        }
    }
    assert!(bad.is_empty(), "negative controls triggered: {bad:?}");
}
