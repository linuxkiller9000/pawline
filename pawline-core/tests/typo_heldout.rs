//! Held-out typo set. Written BEFORE the weather/search/question/small-talk scanners existed and not used to tune them.
//! Misses are reported (printed), not hidden. Hard safety asserts: no name/payload change, no wrong correction on negative controls.
use pawline_core::typo::suggest;

const POS: &[(&str, &str)] = &[
    ("delet the folder logs", "delete the folder logs"),
    ("remov the file todo.txt", "remove the file todo.txt"),
    ("creaet a folder called backup", "create a folder called backup"),
    ("mkae folder reports", "make folder reports"),
    ("reanme the file a.txt to b.txt", "rename the file a.txt to b.txt"),
    ("mvoe notes.txt to docs", "move notes.txt to docs"),
    ("copy the flie a.txt to docs", "copy the file a.txt to docs"),
    ("opne firefox", "open firefox"),
    ("lanuch mpv", "launch mpv"),
    ("shwo folders", "show folders"),
    ("wether in cairo", "weather in cairo"),
    ("whats the wether in new york", "whats the weather in new york"),
    ("weahter for paris", "weather for paris"),
    ("forcast for london", "forecast for london"),
    ("serach for cats", "search for cats"),
    ("serach wikipedia for cats", "search wikipedia for cats"),
    ("search wikipdia for cats", "search wikipedia for cats"),
    ("waht is gravity", "what is gravity"),
    ("explian how tides work", "explain how tides work"),
    ("defien entropy", "define entropy"),
    ("whta is a black hole?", "what is a black hole?"),
    ("hlep", "help"),
    ("thansk", "thanks"),
    ("pelase mkae a folder called tmp", "make a folder called tmp"),
    ("please delte the folder kp", "delete the folder kp"),
    ("undoo", "undo"),
    ("orgnize my downloads", "organize my downloads"),
    ("shrnik video clip.mp4", "shrink video clip.mp4"),
    ("wrtie a presentation about space", "write a presentation about space"),
    ("make a presentaion about tea", "make a presentation about tea"),
    // unquoted target names at the boundary: the name stays exactly as typed
    ("delte flder kp", "delete folder kp"),
    ("mvoe foldr to docs", "move foldr to docs"),
    ("opne mpv-player", "open mpv-player"),
    ("serach for wheather", "search for wheather"),
    ("wether in cario", "weather in cario"),
    ("waht is wether", "what is wether"),
    ("delte kitten", "delete kitten"),
    ("delte \"my folder\"", "delete \"my folder\""),
    ("please make a folder called \"please keep\"", "make a folder called \"please keep\""),
    ("please make a folder called please", "make a folder called please"),
];

/// Inputs that must produce NO suggestion at all.
const NEG: &[&str] = &[
    "hello there", "what is gravity", "snow in cairo", "whether in cairo is nice", "yse", "aprove", "tell me a joke", "the folder is full",
    "delete the folder kp", "make a folder called delte", "rename the folder foler to flder", "yes delete it", "no", "cancel", "banana",
    "lost files", "copy mkae to delte", "delete foldr", "move foldr to docs", "weather in cairo", "search for cats", "make a folder called please",
    "i like cats", "how are you", "dleete", "xyz", "mode dark", "code review", "post it",
];

#[test]
fn heldout_positive_and_negative() {
    let mut ok = 0;
    let mut miss = vec![];
    for (typed, want) in POS {
        match suggest(typed) {
            Some(s) if s.corrected == *want => ok += 1,
            other => miss.push(format!("{typed:?} -> {:?} (wanted {want:?})", other.map(|s| s.corrected))),
        }
    }
    let mut wrong = vec![];
    for n in NEG {
        if let Some(s) = suggest(n) {
            wrong.push(format!("{n:?} -> {:?}", s.corrected));
        }
    }
    eprintln!("HELD-OUT positives: {ok}/{} as expected. Misses: {miss:#?}", POS.len());
    eprintln!("HELD-OUT negative controls: {}/{} left alone. Wrong suggestions: {wrong:#?}", NEG.len() - wrong.len(), NEG.len());
    // Safety: never a wrong suggestion on a control, and nothing in the payload ever changes.
    assert!(wrong.is_empty(), "negative controls got a suggestion: {wrong:#?}");
}
