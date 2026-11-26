//! Offline conversation: small talk, usage hints and jokes. Local, deterministic, no model, no network.
//!
//! Safety rules this module enforces for the app:
//! - Discussing an action is not requesting it. Questions about how to do something ("how do I delete a folder?"),
//!   hypotheticals ("what if I delete kp"), negations ("don't delete kp") and past tense never reach the planner or the
//!   on-device model, and never run anything. A polite or wishing request ("can you delete the folder kp", "I want to delete
//!   the folder kp") is turned into the exact command as a clickable pick: nothing runs until the user sends that command,
//!   and the normal confirm card still follows.
//! - Replies are plain text. Nothing here reads files, history or location, and nothing leaves the machine.
//! - Conversation memory is a few recent lines kept in memory only (never written to disk) and cleared on request.
//! - Typos in the chat words are understood (small edit distance on words of 4+ letters). Payload words are never rewritten.

use crate::typo::distance;
use std::collections::VecDeque;

pub const MAX_TURNS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Topic {
    Joke,
    Hint,
}

#[derive(Debug, Default)]
pub struct ChatState {
    pub last: Option<Topic>,
    joke_i: usize,
    hint_i: usize,
    /// (who, text) with who = "you" | "pawline". Memory only.
    pub turns: VecDeque<(&'static str, String)>,
}

/// What the chat layer decided for an input.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Not conversation: carry on with commands, questions and everything else, unchanged.
    Pass,
    /// A plain text reply. Nothing runs.
    Reply(String),
    /// A reply plus the exact command the user can pick (it is NOT run; it still needs sending and the confirm card).
    Pick { text: String, command: String },
    /// The input talks about an action but is not a request for it. Nothing runs, the model is not consulted.
    Discussion(String),
}

pub const JOKES: &[&str] = &[
    "Why do programmers prefer dark mode? Because light attracts bugs.",
    "I would tell you a UDP joke, but you might not get it.",
    "There are 10 kinds of people: those who understand binary and those who don't.",
    "Why did the folder go to therapy? It had too many unresolved subfolders.",
    "A SQL query walks into a bar, sees two tables and asks: can I join you?",
    "Why was the file so calm? It knew how to handle exceptions.",
    "I told my computer I needed a break. It said: no problem, I'll go to sleep.",
    "Why did the developer go broke? They used up all their cache.",
    "What do you call a fake noodle? An impasta.",
    "Why don't scientists trust atoms? They make up everything.",
    "I'm reading a book about anti-gravity. It's impossible to put down.",
    "What's a cat's favourite command? sudo purr.",
];

/// (keywords, example command, hint text). The example commands are checked against the real parsers by a test, so hints never drift from the app.
pub const HINTS: &[(&[&str], &str, &str)] = &[
    (&["make", "create", "new", "folder", "file", "mkdir"], "make a folder called kitten paws", "Make a folder or file: \"make a folder called kitten paws\" or \"make a file called notes.txt\"."),
    (&["delete", "remove", "trash", "erase"], "delete the folder kitten paws", "Delete: \"delete the folder kitten paws\". I always show the full path and ask first, and \"undo\" can put it back."),
    (&["rename"], "rename the file a.txt to b.txt", "Rename: \"rename the file a.txt to b.txt\". It asks first."),
    (&["move"], "move the file a.txt to docs", "Move: \"move the file a.txt to docs\". It asks first and never overwrites."),
    (&["copy"], "copy the file a.txt to docs", "Copy: \"copy the file a.txt to docs\"."),
    (&["list", "show"], "list the folder docs", "List what is in a folder: \"list the folder docs\"."),
    (&["open", "launch", "app", "start"], "open mpv", "Open an app: \"open mpv\". I show the exact command and ask first."),
    (&["wallpaper", "background"], "set wallpaper to ~/Pictures/a.png", "Wallpaper: \"set wallpaper to ~/Pictures/a.png\". It asks first and can be undone."),
    (&["search", "google", "lookup"], "search for what is a cat", "Search: \"search for what is a cat\" or \"search wikipedia for cats\". It asks before going online."),
    (&["weather", "forecast", "temperature"], "weather in Cairo", "Weather: \"weather in Cairo\". It asks before going online."),
    (&["math", "calculate", "solve", "plus", "times"], "what is 12 * 7", "Math: \"what is 12 * 7\" or \"solve 2x + 3 = 11\". It is computed here, never guessed."),
    (&["undo", "revert"], "undo", "Undo the last change: \"undo\"."),
    (&["presentation", "slides", "slideshow", "deck"], "make a presentation from notes.txt", "Slides: \"make a presentation from notes.txt\", or \"write a presentation about the water cycle\" (online, you review the draft before anything is saved)."),
    (&["video", "shrink", "compress"], "shrink video clip.mp4", "Shrink a video: \"shrink video clip.mp4\". It shows the plan first."),
    (&["organize", "organise", "sort", "tidy"], "organize my downloads", "Tidy a folder: \"organize my downloads\". It shows every step first."),
    (&["find", "locate"], "fd report", "Find files by name: \"fd report\". Find text inside files: \"rg budget\"."),
];

fn norm(input: &str) -> String {
    let t: String = input
        .to_lowercase()
        .replace('\u{2019}', "'")
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '\'' || c.is_whitespace() { c } else { ' ' })
        .collect();
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Does the word match the token, allowing a small typo for words of 4+ letters?
fn tok_is(tok: &str, word: &str) -> bool {
    if tok == word {
        return true;
    }
    let n = word.chars().count();
    if n == 4 {
        // 4-letter words: only a swap of neighbouring letters ("joek", "hnit"), so "bake" is not "make" and "coke" is not "joke".
        let (mut a, mut b): (Vec<char>, Vec<char>) = (tok.chars().collect(), word.chars().collect());
        a.sort();
        b.sort();
        return a == b && distance(tok, word) == 1;
    }
    n >= 5 && tok.chars().count() >= 4 && distance(tok, word) <= if n >= 8 { 2 } else { 1 }
}

fn has_word(toks: &[&str], word: &str) -> bool {
    toks.iter().any(|t| tok_is(t, word))
}

fn starts_with_phrase(toks: &[&str], phrase: &str) -> bool {
    let p: Vec<&str> = phrase.split(' ').collect();
    toks.len() >= p.len() && p.iter().enumerate().all(|(i, w)| toks[i] == *w)
}

const ACTION_WORDS: &[&str] = &[
    "make", "create", "delete", "remove", "trash", "rename", "move", "copy", "open", "launch", "set", "change", "undo", "write", "organize", "organise",
    "shrink", "compress", "erase", "list", "show", "run", "install", "uninstall",
];

fn is_action_word(t: &str) -> bool {
    ACTION_WORDS.contains(&t) || (t.chars().count() >= 4 && ACTION_WORDS.iter().any(|w| w.chars().count() >= 4 && distance(t, w) <= 1))
}

fn first_action_index(toks: &[&str]) -> Option<usize> {
    toks.iter().position(|t| is_action_word(t))
}

/// "don't delete kp", "never delete ...", "do not move it", "stop deleting things": a negation BEFORE the action word.
/// A real command has its action word first, so "make a folder called not" is never caught.
fn is_negated_action(toks: &[&str]) -> bool {
    let neg = |t: &&str| matches!(*t, "dont" | "don't" | "dnt" | "not" | "never" | "stop" | "without" | "avoid") || tok_is(t, "dont");
    if let Some(i) = first_action_index(toks) {
        return i > 0 && toks[..i].iter().any(neg);
    }
    toks.len() <= 5 && toks.iter().any(neg) && toks.iter().any(|t| t.ends_with("ing") && is_action_word(t.trim_end_matches("ing")))
}

const FILLERS: &[&str] = &["hey", "hi", "pawline", "please", "pls", "plz", "ok", "okay", "um", "so", "well", "and"];

/// Lead phrase matched at the start (after fillers). Returns how many raw tokens the lead (with fillers) covers.
fn lead_len(toks: &[&str], leads: &[&str]) -> Option<usize> {
    let mut i = 0;
    while i < toks.len() && FILLERS.contains(&toks[i]) {
        i += 1;
    }
    leads.iter().filter(|l| starts_with_phrase(&toks[i..], l)).map(|l| i + l.split(' ').count()).max()
}

/// The hint for the first word (in the order typed) that names a topic: "rename a file" is about rename, not file.
fn hint_for(words: &[&str]) -> Option<&'static (&'static [&'static str], &'static str, &'static str)> {
    words.iter().find_map(|w| HINTS.iter().find(|h| h.0.iter().any(|k| tok_is(w, k))))
}

/// The raw words after the first `skip` tokens, joined, without trailing sentence punctuation and a trailing "please".
fn raw_rest(input: &str, skip: usize) -> String {
    let raw: Vec<&str> = input.split_whitespace().collect();
    let mut rest: Vec<&str> = raw.iter().skip(skip).copied().collect();
    if let Some(last) = rest.last_mut() {
        *last = last.trim_end_matches(['?', '.', '!']);
    }
    while rest.last().is_some_and(|w| w.is_empty()) {
        rest.pop();
    }
    if rest.len() > 1 {
        if let Some(l) = rest.last() {
            if matches!(l.to_lowercase().trim_end_matches(','), "please" | "pls" | "plz") && !l.starts_with(['"', '\'']) {
                rest.pop();
            }
        }
    }
    rest.join(" ")
}

/// "make a folder called x please" -> "make a folder called x". A trailing please/pls/thanks is politeness, not part of a name.
/// Never for a quoted name, and never right after called/named/folder/file/a/the/to (then it may be the name itself).
pub fn strip_trailing_polite(input: &str) -> Option<String> {
    let t = input.trim().trim_end_matches(['.', '!', '?']);
    let (head, last) = t.rsplit_once(char::is_whitespace)?;
    let last_l = last.trim_end_matches(',').to_lowercase();
    if !matches!(last_l.as_str(), "please" | "pls" | "plz") || last.starts_with(['"', '\'']) || last.ends_with(['"', '\'']) {
        return None;
    }
    let head = head.trim_end().trim_end_matches(',').trim_end();
    let prev = head.rsplit(char::is_whitespace).next().unwrap_or("").to_lowercase();
    if matches!(prev.as_str(), "called" | "named" | "folder" | "file" | "directory" | "a" | "an" | "the" | "to" | "into" | "in" | "inside" | "from" | "and") || head.ends_with(['"', '\'']) {
        return None;
    }
    Some(head.to_string())
}

impl ChatState {
    pub fn forget(&mut self) {
        self.turns.clear();
        self.last = None;
    }

    fn push(&mut self, who: &'static str, text: &str) {
        self.turns.push_back((who, text.chars().take(200).collect()));
        while self.turns.len() > MAX_TURNS {
            self.turns.pop_front();
        }
    }

    fn joke(&mut self) -> String {
        let j = JOKES[self.joke_i % JOKES.len()];
        self.joke_i += 1;
        self.last = Some(Topic::Joke);
        j.to_string()
    }

    fn hint(&mut self) -> String {
        let h = HINTS[self.hint_i % HINTS.len()].2;
        self.hint_i += 1;
        self.last = Some(Topic::Hint);
        format!("Hint: {h}")
    }

    /// Decide what to do with an input. `is_command` tells whether Pawline's own parsers already understand it as a command
    /// (then the chat layer stays out of the way, unless the sentence is a negated action: that is never a command).
    pub fn respond(&mut self, input: &str, is_command: bool) -> Outcome {
        let n = norm(input);
        if n.is_empty() || input.trim_start().starts_with('/') {
            return Outcome::Pass;
        }
        let toks: Vec<&str> = n.split_whitespace().collect();
        let out = self.decide(input, &n, &toks, is_command);
        let forgot = matches!(n.as_str(), "forget our chat" | "clear our chat" | "forget this chat" | "forget our conversation");
        match &out {
            _ if forgot => {}
            Outcome::Pass => {}
            Outcome::Reply(t) | Outcome::Discussion(t) | Outcome::Pick { text: t, .. } => {
                self.push("you", input);
                self.push("pawline", t);
            }
        }
        out
    }

    fn decide(&mut self, input: &str, n: &str, toks: &[&str], is_command: bool) -> Outcome {
        // 1. Negated actions are never run.
        if is_negated_action(toks) {
            return Outcome::Discussion("Okay, I won't. Nothing was done.".into());
        }
        if is_command {
            return Outcome::Pass;
        }
        // 2. Questions about HOW to do something: explain, never act.
        if let Some(k) = lead_len(toks, &["how do i", "how can i", "how should i", "how would i", "how to", "how do you", "how do we"]) {
            return match hint_for(&toks[k..]) {
                Some(h) => {
                    self.last = Some(Topic::Hint);
                    Outcome::Discussion(format!("{} Nothing was done. To try it, send: {}", h.2, h.1))
                }
                None => Outcome::Pass, // e.g. "how to bake bread": an ordinary question, handled as before
            };
        }
        // 3. Hypotheticals and advice: talk about it, never act.
        if let Some(k) = lead_len(
            toks,
            &["what if i", "what if", "what happens if", "what happens when", "is it safe to", "is it ok to", "is it okay to", "should i", "do i need to", "would it", "will it", "what would happen"],
        ) {
            if first_action_index(&toks[k..]).is_some() {
                return Outcome::Discussion(
                    "That is a what-if, so I did nothing. When you do ask me to change something, I show the full path and wait for your Do it first, and \"undo\" can reverse the last change. Say the command itself when you want it done.".into(),
                );
            }
        }
        // 4. Polite or wishing requests: offer the exact command as a pick. Nothing runs.
        if let Some(k) = lead_len(toks, &["can you", "could you", "would you", "will you", "i want to", "i wanna", "i need to", "i'd like to", "id like to", "i would like to", "lets", "let's", "go ahead and"]) {
            let first = toks.get(k).copied().unwrap_or("");
            if is_action_word(first) {
                let rest = raw_rest(input, k_raw(input, k));
                let mut cmd = rest.clone();
                if let Some(w) = rest.split_whitespace().next() {
                    let lw = w.to_lowercase();
                    if !crate::typo::RISKY_VERBS.contains(&lw.as_str()) && !crate::typo::SAFE_VERBS.contains(&lw.as_str()) {
                        if let Some(v) = crate::typo::RISKY_VERBS.iter().chain(crate::typo::SAFE_VERBS).find(|v| tok_is(&lw, v)) {
                            cmd = format!("{v}{}", &rest[w.len()..]);
                        }
                    }
                }
                let generic = ["folder", "folders", "file", "files", "it", "them", "that", "this", "things", "stuff", "something", "anything", "everything", "a", "an", "the", "my", "some"];
                let target_words: Vec<String> = cmd.split_whitespace().skip(1).map(|w| w.to_lowercase()).collect();
                let has_target = target_words.iter().any(|w| !generic.contains(&w.as_str()));
                if has_target && crate::typo::recognised_cmd(&cmd) {
                    return Outcome::Pick {
                        text: format!("Do you want me to do this? Pick it to put it in the box, then press Enter. Nothing has been done yet, and a card will ask before anything changes:\n{cmd}"),
                        command: cmd,
                    };
                }
                return Outcome::Reply("I can do that, but I need it as a command with a name, for example: \"delete the folder kitten paws\". Say \"hint\" for more examples. Nothing was done.".into());
            }
        }
        // 5. Past tense and questions about what I did.
        if toks.first().is_some_and(|t| matches!(*t, "i" | "we" | "he" | "she" | "they"))
            && toks.iter().skip(1).take(3).any(|t| matches!(*t, "deleted" | "removed" | "moved" | "copied" | "renamed" | "made" | "created" | "opened"))
        {
            return Outcome::Reply("Got it. I only change things when you tell me to and confirm, so nothing happened here. Need anything done?".into());
        }
        if lead_len(toks, &["why did you", "did you", "have you", "what did you do", "what have you done"]).is_some() {
            return Outcome::Reply("I only change things after you press Do it on a card. If something looks wrong, say \"undo\" to reverse the last change.".into());
        }
        // 6. Follow-ups.
        if matches!(
            n,
            "another" | "another one" | "one more" | "again" | "more" | "next" | "next one" | "and another" | "tell me another" | "another joke" | "more jokes" | "more hints" | "another hint" | "again please"
        ) {
            return match self.last {
                Some(Topic::Joke) => Outcome::Reply(self.joke()),
                Some(Topic::Hint) => Outcome::Reply(self.hint()),
                None => Outcome::Reply("Another what? I can do a joke or a hint. Say \"joke\" or \"hint\".".into()),
            };
        }
        if (toks.len() <= 6 && (has_word(toks, "joke") || has_word(toks, "jokes"))) || n.contains("make me laugh") || n.contains("something funny") {
            return Outcome::Reply(self.joke());
        }
        if (toks.len() <= 4 && (has_word(toks, "hint") || has_word(toks, "hints") || has_word(toks, "tips") || has_word(toks, "tip")))
            || n.contains("give me a hint")
            || n.contains("any tips")
            || n.contains("how do i use you")
            || n.contains("how do i use this")
            || n.contains("how should i use")
        {
            return Outcome::Reply(self.hint());
        }
        if n.contains("what did i just") || n.contains("what did i say") || n.contains("what were we") {
            let last_you = self.turns.iter().rev().filter(|t| t.0 == "you").nth(1).map(|t| t.1.clone());
            return Outcome::Reply(match last_you {
                Some(t) => format!("Your last message before this one was: \"{t}\" (I only keep the last few lines in memory, never on disk)."),
                None => "Nothing before this in this window. (I only keep the last few lines in memory, never on disk.)".into(),
            });
        }
        if matches!(n, "forget our chat" | "clear our chat" | "forget this chat" | "forget our conversation") {
            self.forget();
            return Outcome::Reply("Forgot our chat in this window. Nothing was stored on disk.".into());
        }
        // 7. Small talk.
        let short = toks.len() <= 6;
        if short
            && (n.contains("how are you") || n.contains("how r u") || n.contains("how are u") || n == "how are things" || n.contains("how's it going") || n.contains("hows it going") || n == "what's up" || n == "whats up" || n == "sup")
        {
            return Outcome::Reply("Doing fine, thanks for asking. Ready when you are. I can make and tidy files, open apps, check the weather, search, do math and make slides. Say \"hint\" for an idea or \"joke\" for a laugh.".into());
        }
        if short && (n.contains("who are you") || n.contains("what are you") || n.contains("your name") || n.contains("what is pawline") || n.contains("what's pawline")) {
            return Outcome::Reply("I'm Pawline, a small assistant for this computer. I do files, apps, weather, search, math and slides here, and I ask before I change anything. I can chat a little offline, and open conversation online needs your OK first.".into());
        }
        if short && (toks.iter().any(|t| matches!(*t, "thanks" | "thank" | "thansk" | "thnks" | "thanx" | "thnx" | "thankyou" | "thanls" | "thamks" | "tanks")) || n == "thx" || n == "ty" || n.contains("thank you") || n.contains("cheers")) && !toks.iter().any(|t| is_action_word(t)) {
            return Outcome::Reply("You're welcome.".into());
        }
        if short && (matches!(n, "bye" | "goodbye" | "good bye" | "see you" | "see ya" | "later" | "good night" | "goodnight" | "night" | "cya") || has_word(toks, "goodbye")) {
            return Outcome::Reply("Bye for now. I'll be here.".into());
        }
        if short && (n.contains("i'm bored") || n.contains("im bored") || n == "bored" || n.contains("i am bored")) {
            let j = self.joke();
            return Outcome::Reply(format!("Let me help with that: {j}\nSay \"another\" for more, or \"hint\" for something useful."));
        }
        if short && (n.contains("good job") || n.contains("well done") || n.contains("you're great") || n.contains("youre great") || n.contains("nice work") || n.contains("love you") || n.contains("you are great")) {
            return Outcome::Reply("Thank you, that's kind. Anything else to do?".into());
        }
        if short && (n.starts_with("sorry") || n.contains("my bad") || n.contains("my mistake")) {
            return Outcome::Reply("No problem at all.".into());
        }
        if short && matches!(n, "lol" | "haha" | "hahaha" | "lmao" | "funny" | "nice" | "cool" | "ok" | "okay" | "great" | "wow") {
            return Outcome::Reply(if self.last == Some(Topic::Joke) { "Glad that landed. Say \"another\" for one more.".into() } else { "Okay.".into() });
        }
        Outcome::Pass
    }
}

/// The normalized token index `k` counts words after stripping punctuation; map it to the raw whitespace-token index.
/// Normalization can split a raw word ("kp," stays one word, "a.txt" becomes two), so count raw tokens until `k` normalized ones are covered.
fn k_raw(input: &str, k: usize) -> usize {
    let mut seen = 0usize;
    for (i, w) in input.split_whitespace().enumerate() {
        if seen >= k {
            return i;
        }
        seen += norm(w).split_whitespace().count().max(1);
    }
    input.split_whitespace().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(c: &mut ChatState, s: &str) -> Outcome {
        c.respond(s, crate::typo::recognised_cmd(s))
    }
    fn is_text(o: &Outcome) -> bool {
        matches!(o, Outcome::Reply(_) | Outcome::Discussion(_))
    }

    #[test]
    fn ordinary_sentences_are_not_small_talk() {
        let mut c = ChatState::default();
        for s in ["I think clouds look like sheep", "I think so", "thin clouds today", "the tank is full"] {
            assert_eq!(c.respond(s, false), Outcome::Pass, "{s}");
        }
    }

    #[test]
    fn hint_examples_are_real_commands() {
        for (_, ex, _) in HINTS {
            assert!(crate::typo::recognised_cmd(ex) || crate::textfallback::classify(ex) != crate::textfallback::Class::No, "hint example not understood by the parsers: {ex}");
        }
    }

    #[test]
    fn discussing_an_action_never_acts_and_never_reaches_the_planner() {
        let mut c = ChatState::default();
        for s in [
            "how do I delete a folder?",
            "how can i delete the folder kp",
            "how to make a folder",
            "what if I delete the folder kp",
            "should i delete the folder kp?",
            "is it safe to delete kp",
            "don't delete the folder kp",
            "do not delete kp",
            "never move the file a.txt",
            "dont delte the folder kp",
            "stop deleting things",
            "please don't delete kp",
            "i deleted a folder yesterday",
            "why did you delete kp",
        ] {
            let o = r(&mut c, s);
            assert!(is_text(&o), "{s:?} -> {o:?}");
        }
    }

    #[test]
    fn a_polite_request_is_a_pick_with_the_name_exactly_as_typed() {
        let mut c = ChatState::default();
        match r(&mut c, "can you delete the folder Kitten Paws?") {
            Outcome::Pick { command, .. } => assert_eq!(command, "delete the folder Kitten Paws"),
            o => panic!("{o:?}"),
        }
        match r(&mut c, "I want to make a folder called pics please") {
            Outcome::Pick { command, .. } => assert_eq!(command, "make a folder called pics"),
            o => panic!("{o:?}"),
        }
        match r(&mut c, "could you delte the folder kp") {
            Outcome::Pick { command, .. } => assert_eq!(command, "delete the folder kp"),
            o => panic!("{o:?}"),
        }
        match r(&mut c, "can you move notes.txt to docs") {
            Outcome::Pick { command, .. } => assert_eq!(command, "move notes.txt to docs"),
            o => panic!("{o:?}"),
        }
        // a quoted name that ends with the word please keeps it
        match r(&mut c, "can you make a folder called \"say please\"") {
            Outcome::Pick { command, .. } => assert_eq!(command, "make a folder called \"say please\""),
            o => panic!("{o:?}"),
        }
        // no usable target: explained, nothing offered
        assert!(matches!(r(&mut c, "can you delete folders?"), Outcome::Reply(t) if t.contains("Nothing was done")));
    }

    #[test]
    fn plain_commands_and_general_questions_pass_through() {
        let mut c = ChatState::default();
        for s in ["delete the folder kp", "make a folder called not", "make a folder called dont-use", "what is gravity", "how to bake bread", "weather in cairo", "how does gravity work"] {
            assert_eq!(r(&mut c, s), Outcome::Pass, "{s:?}");
        }
    }

    #[test]
    fn jokes_hints_followups_and_typos() {
        let mut c = ChatState::default();
        let a = match r(&mut c, "tell me a joke") {
            Outcome::Reply(t) => t,
            o => panic!("{o:?}"),
        };
        let b = match r(&mut c, "another one") {
            Outcome::Reply(t) => t,
            o => panic!("{o:?}"),
        };
        assert_ne!(a, b);
        assert!(JOKES.contains(&a.as_str()) && JOKES.contains(&b.as_str()));
        assert!(matches!(r(&mut c, "tel me a joek"), Outcome::Reply(_)));
        assert!(matches!(r(&mut c, "give me a hnit"), Outcome::Reply(t) if t.starts_with("Hint:")));
        assert!(matches!(r(&mut c, "thansk"), Outcome::Reply(t) if t == "You're welcome."));
        let _ = r(&mut c, "hint");
        assert!(matches!(r(&mut c, "another"), Outcome::Reply(t) if t.starts_with("Hint:")));
        let mut d = ChatState::default();
        assert!(matches!(r(&mut d, "another"), Outcome::Reply(t) if t.contains("Another what")));
    }

    #[test]
    fn how_do_i_gives_the_matching_topic_and_does_nothing() {
        let mut c = ChatState::default();
        match r(&mut c, "how do i rename a file?") {
            Outcome::Discussion(t) => assert!(t.contains("rename the file a.txt to b.txt") && t.contains("Nothing was done")),
            o => panic!("{o:?}"),
        }
        match r(&mut c, "how do I set a wallpaper") {
            Outcome::Discussion(t) => assert!(t.contains("allpaper")),
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn memory_is_bounded_in_memory_and_forgettable() {
        let mut c = ChatState::default();
        for i in 0..20 {
            let _ = r(&mut c, if i % 2 == 0 { "thanks" } else { "tell me a joke" });
        }
        assert!(c.turns.len() <= MAX_TURNS);
        assert!(matches!(r(&mut c, "what did I just say"), Outcome::Reply(t) if t.contains("last message before")));
        assert!(matches!(r(&mut c, "forget our chat"), Outcome::Reply(_)));
        assert!(c.turns.is_empty());
    }

    #[test]
    fn trailing_please_is_politeness_except_in_names() {
        assert_eq!(strip_trailing_polite("make a folder called x please").as_deref(), Some("make a folder called x"));
        assert_eq!(strip_trailing_polite("delete the folder kp, please!").as_deref(), Some("delete the folder kp"));
        assert_eq!(strip_trailing_polite("make a folder called please"), None);
        assert_eq!(strip_trailing_polite("make a folder called \"say please\""), None);
        assert_eq!(strip_trailing_polite("make a folder called 'x' please"), None);
        assert_eq!(strip_trailing_polite("delete the file please"), None);
        assert_eq!(strip_trailing_polite("make a folder called x"), None);
    }

    #[test]
    fn small_talk_basics() {
        let mut c = ChatState::default();
        for s in ["how are you", "who are you", "bye", "i'm bored", "good job", "sorry", "lol"] {
            assert!(is_text(&r(&mut c, s)), "{s}");
        }
    }
}
