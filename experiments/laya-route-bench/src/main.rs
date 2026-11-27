// Laya routing-quality harness: the REAL converted model + REAL tokenizer +
// the routing code copied verbatim from pawline-native/src/laya.rs (constants
// and helpers are extracted mechanically; route_inner is a faithful port).
use tract_onnx::prelude::*;
use tract_onnx::tract_core::plan::SimplePlan;
type Plan = SimplePlan<TypedFact, Box<dyn TypedOp>>;


const INTENTS: &[(&str, &[&str])] = &[
    ("make_folder", &["make a folder", "create a new directory"]),
    ("make_file", &["make a file", "create a new file"]),
    ("delete", &["delete a file or folder", "remove something", "trash this file"]),
    ("rename", &["rename a file or folder", "change the name of a file"]),
    ("list_files", &["list the files in a folder", "show me what is in downloads"]),
    ("open_app", &["open an application", "launch firefox", "start a program", "open the calculator"]),
    ("weather", &["what is the weather", "will it rain", "weather forecast for a city", "is it going to be hot", "do i need a coat today"]),
    ("math", &["calculate something", "solve an equation", "what is 2 plus 2", "solve for x"]),
    ("search", &["search for information", "look something up", "what is a black hole", "what is a file system", "how do magnets work", "why is the sky blue", "a general knowledge question"]),
    ("remind", &["remind me to do something", "set a reminder", "i keep forgetting to do something", "i need to remember to", "nudge me later"]),
    ("volume", &["turn up the volume", "set volume to 50", "mute the sound", "i cant hear anything", "the sound is too quiet", "make it louder"]),
    ("help", &["what can you do", "show me examples", "help me use you"]),
    // Decoys: absorb not-a-command talk that sits near command words. Treated as no match.
    ("chat_about_words", &["asking what a computing word means", "what does the word delete mean", "talking about a key on the keyboard", "discussing technology in the abstract"]),
    ("just_chatting", &["a story or opinion, not a request", "mentioning something without asking for it to be done", "talking about the weather in books or films"]),
];

/// Cheap keyword-family shortlist: the model only ever sees the few intents a message
/// can plausibly mean, plus both decoys and the none path. Keeps sequences short
/// (faster) and quality high (full phrasings fit the token window). Validated against
/// the 52-line battery: 9 benign misses, zero dangerous false accepts.
const FAMILIES: &[(&str, &[&str])] = &[
    (r"\b(folder|directory|mkdir)\b", &["make_folder"]),
    (r"\b(file|document|note)\b", &["make_file", "list_files"]),
    (r"\b(delet|remov|trash|erase|wipe)\w*", &["delete"]),
    (r"\b(rename|call it|change the name)\b", &["rename"]),
    (r"\b(list|show|what'?s in|downloaded|contents)\b", &["list_files"]),
    (r"\b(open|launch|start|run|firefox|spotify|app)\b", &["open_app"]),
    (r"\b(weather|rain|hot|cold|coat|jacket|forecast|umbrella|sunny|cloudy|wether|snow|wind|windy|fog|storm|degrees|temperature|warm)\b", &["weather"]),
    (r"\b(calculat|solve|equation|plus|minus|times|divided|math|percent)\w*|\bhow (much|many)\b|\d\s*[-+*/^=]\s*\d|\d+x", &["math"]),
    (r"\b(what|why|how|who|explain|mean|means|meaning|is a|are a)\b|\blook ?up\b|\btell me about\b", &["search", "chat_about_words"]),
    (r"\b(remind|remember|forget|nudge)\w*", &["remind"]),
    (r"\b(volume|sound|hear|loud\w*|quiet|mute|muting|deaf)\b", &["volume"]),
    (r"\b(help|examples|what can you do|use this|how do i use)\b", &["help"]),
];

const NONE_KEY: &str = "__none__";
const NONE_DESC: &str = "Gibberish, filler words, or a message unrelated to every other path.";
const INSTRUCTIONS: &str = "A user sent this message to a conversational workflow that branches into the paths below. Which path is the message asking for?";
const MAX_LEN: usize = 512;
const HEAD_MAX_LEN: usize = 384;
const OPTION_CAP: usize = 96;
const MIN_HEAD_TOKENS: usize = 16;
const PHRASING_SEP: &str = "\" | \"";
/// Non-destructive intents need this much confidence; destructive ones more (a confirm
/// card still follows either way - the threshold is about not *offering* nonsense).
const THRESHOLD: f32 = 0.96;
const THRESHOLD_DESTRUCTIVE: f32 = 0.96;
const CACHE_MAX: usize = 64;

fn temperature(n: usize) -> f32 {
    match n {
        0..=2 => 1.1014399528503418,
        3..=5 => 1.1109503507614136,
        6..=10 => 1.1113102436065674,
        _ => 1.0,
    }
}

fn water_fill(lengths: &[usize], mut budget: usize, floor: usize) -> Vec<usize> {
    let mut open: Vec<usize> = (0..lengths.len()).collect();
    while !open.is_empty() {
        let fair = budget / open.len();
        let short: Vec<usize> = open.iter().cloned().filter(|&i| lengths[i] <= fair).collect();
        if short.is_empty() {
            break;
        }
        budget -= short.iter().map(|&i| lengths[i]).sum::<usize>();
        open.retain(|&i| lengths[i] > fair);
    }
    let mut alloc = lengths.to_vec();
    for (j, &i) in open.iter().enumerate() {
        alloc[i] = (budget / open.len() + usize::from(j < budget % open.len())).max(floor);
    }
    alloc
}

fn json_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}


fn main() {
    let args: Vec<String> = std::env::args().collect();
    let model_path = args.get(1).map(String::as_str).unwrap_or("/downloads/laya/laya_fp32.onnx");
    let tok_path = args.get(2).map(String::as_str).unwrap_or("/downloads/laya/tokenizer.json");
    let tok = tokenizers::Tokenizer::from_file(tok_path).unwrap();
    let (cls, sep, mask) = (
        tok.token_to_id("[CLS]").unwrap() as i64,
        tok.token_to_id("[SEP]").unwrap() as i64,
        tok.token_to_id("[MASK]").unwrap() as i64,
    );
    let plan = tract_onnx::onnx()
        .model_for_path(model_path)
        .unwrap().into_optimized().unwrap().into_runnable().unwrap();
    let plan = std::sync::Arc::new(plan);

    let enc = |s: &str| -> (Vec<u32>, Vec<(usize, usize)>) {
        let e = tok.encode(s.replace("[MASK]", " "), false).unwrap();
        (e.get_ids().to_vec(), e.get_offsets().to_vec())
    };

    // (message, expected intent or "__none__" for "no suggestion")
    let battery: &[(&str, &str)] = &[
        ("make a folder called kp", "make_folder"),
        ("create a new directory named stuff", "make_folder"),
        ("delete the folder kp", "delete"),
        ("trash this file please", "delete"),
        ("rename report.txt to final.txt", "rename"),
        ("list the files in downloads", "list_files"),
        ("open firefox", "open_app"),
        ("what is the weather in cairo", "weather"),
        ("will it rain tomorrow", "weather"),
        ("what is 2 plus 2", "math"),
        ("solve x + 3 = 7", "math"),
        ("what is a black hole", "search"),
        ("why is the sky blue", "search"),
        ("remind me to call mom", "remind"),
        ("turn up the volume", "volume"),
        ("mute the sound", "volume"),
        ("what can you do", "help"),
        // should NOT suggest (decoys / none / no family)
        ("what does the word delete mean", "__none__"),
        ("i was deleting monsters in the game all night", "__none__"),
        ("the weather in that movie was amazing", "__none__"),
        ("hello there", "__none__"),
        ("my folder of memories from school", "__none__"),
        // extended battery: margin study for threshold tuning
        ("new folder named photos", "make_folder"),
        ("make me a file called notes.txt", "make_file"),
        ("remove the old logs", "delete"),
        ("change the name of draft.doc", "rename"),
        ("show me what is in my pictures folder", "list_files"),
        ("launch the calculator", "open_app"),
        ("do i need a jacket tonight", "weather"),
        ("how much is 15 percent of 80", "math"),
        ("look up the capital of iceland", "search"),
        ("nudge me to water the plants at 6", "remind"),
        ("make it louder please", "volume"),
        ("show me some examples", "help"),
        ("this folder has all my childhood drawings", "__none__"),
        ("i deleted my ex's playlist yesterday", "__none__"),
        ("the file cabinet at work is ancient", "__none__"),
        ("the weather in that novel set the mood", "__none__"),
        ("the volume of the container is two liters", "__none__"),
        ("she reminded me of my grandmother", "__none__"),
        ("the search for the missing hiker continues", "__none__"),
        ("he opened up about his feelings", "__none__"),
    ];

    let mut pass = 0;
    let mut fail = 0;
    for (message, want) in battery {
        let t0 = std::time::Instant::now();
        let (got, bestp) = route_inner(&plan, &enc, cls, sep, mask, message);
        let dt = t0.elapsed();
        let name = got.map(|(i, _)| i).unwrap_or("__none__");
        let ok = name == *want;
        if ok { pass += 1 } else { fail += 1 }
        let prob = format!("{bestp:.3}");
        println!("{} {:43} want={:12} got={:12} p={:6} ({:?})", if ok { "PASS" } else { "FAIL" }, format!("{message:?}"), want, name, prob, dt);
    }
    println!("== {pass} pass / {fail} fail of {} ==", battery.len());
}

fn route_inner(
    plan: &std::sync::Arc<Plan>,
    enc: &dyn Fn(&str) -> (Vec<u32>, Vec<(usize, usize)>),
    cls: i64, sep: i64, mask: i64,
    message: &str,
) -> (Option<(&'static str, f32)>, f32) {
    let mut win = narrow(message);
    if win.is_empty() {
        return (None, 0.0);
    }
    for d in ["chat_about_words", "just_chatting"] {
        if !win.contains(&d) {
            win.push(d);
        }
    }
    let mut texts: Vec<String> = Vec::new();
    for name in &win {
        let Some((_, phrasings)) = INTENTS.iter().find(|(n, _)| n == name) else { return (None, 0.0) };
        let joined = phrasings.iter().map(|p| format!("\"{p}\"")).collect::<Vec<_>>().join(PHRASING_SEP);
        texts.push(format!(" {name}: {joined}").replace("[MASK]", " "));
    }
    texts.push(format!(" {NONE_KEY}: {NONE_DESC}").replace("[MASK]", " "));
    let mut options: Vec<Vec<i64>> = Vec::new();
    let mut encs: Vec<(Vec<u32>, Vec<(usize, usize)>)> = texts.iter().map(|t| enc(t)).collect();
    let lengths: Vec<usize> = encs.iter().map(|e| e.0.len()).collect();
    let opt_total: usize = lengths.iter().sum::<usize>() + encs.len();
    let n = encs.len();
    if opt_total > HEAD_MAX_LEN - MIN_HEAD_TOKENS {
        let a = if n > OPTION_CAP { 0 } else { OPTION_CAP - n };
        let alloc = water_fill(&lengths, HEAD_MAX_LEN - MIN_HEAD_TOKENS, 4);
        for i in 0..n {
            let keep = alloc[i].min(a + 1);
            options.push(std::iter::once(mask).chain(encs[i].0.iter().take(keep).map(|&x| x as i64)).collect());
        }
    } else {
        for i in 0..n {
            options.push(std::iter::once(mask).chain(encs[i].0.iter().map(|&x| x as i64)).collect());
        }
    }
    let _ = &mut encs;
    let head_budget = HEAD_MAX_LEN.saturating_sub(options.iter().map(|o| o.len()).sum::<usize>()).max(8);
    let (hids, _) = enc(&format!("choice question: {INSTRUCTIONS}"));
    let head_len = hids.len().min(head_budget);
    let mut ids: Vec<i64> = vec![cls];
    ids.extend(hids[..head_len].iter().map(|&x| x as i64));
    ids.push(sep);
    let mut markers: Vec<i64> = Vec::new();
    for o in &options {
        markers.push(ids.len() as i64);
        ids.extend_from_slice(o);
    }
    ids.push(sep);
    let state = format!("{{\"message\": {}}}", json_quote(message));
    let room = MAX_LEN.saturating_sub(ids.len() + 1);
    let (st, _) = enc(&state);
    ids.extend(st[..st.len().min(room)].iter().map(|&x| x as i64));
    ids.push(sep);
    ids.truncate(MAX_LEN);
    if markers.iter().any(|&m| m as usize >= MAX_LEN) {
        return (None, 0.0);
    }
    let seq = ids.len();
    let nmark = markers.len();
    let maskv: Vec<i64> = vec![1; seq];
    let mmask: Vec<bool> = vec![true; nmark];
    let ok_t = |r: tract_onnx::prelude::TractResult<Tensor>| r.ok();
    let (t0,t1,t2,t3,t4) = match (ok_t(Tensor::from_shape(&[1, seq], &ids)),
          ok_t(Tensor::from_shape(&[1, seq], &maskv)),
          ok_t(Tensor::from_shape(&[1, nmark], &markers)),
          ok_t(Tensor::from_shape(&[1, nmark], &mmask)),
          ok_t(Tensor::from_shape(&[1], &[0i64]))) {
        (Some(a),Some(b),Some(c),Some(d),Some(e)) => (a,b,c,d,e),
        _ => return (None, 0.0),
    };
    let out = match plan.run(tvec![t0.into(),t1.into(),t2.into(),t3.into(),t4.into()]) {
        Ok(o) => o,
        Err(_) => return (None, 0.0),
    };
    let logits = match out[0].to_plain_array_view::<f32>() { Ok(l) => l, Err(_) => return (None, 0.0) };
    let temp = temperature(nmark);
    let mx = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let exps: Vec<f32> = logits.iter().map(|x| ((x - mx) / temp).exp()).collect();
    let sum: f32 = exps.iter().sum();
    let probs: Vec<f32> = exps.iter().map(|x| x / sum).collect();
    let (bi, bp) = match probs.iter().enumerate().max_by(|a, b| a.1.partial_cmp(b.1).unwrap()).map(|(i, p)| (i, *p)) {
        Some(x) => x,
        None => return (None, 0.0),
    };
    if bi == probs.len() - 1 {
        return (None, bp);
    }
    let intent: &'static str = win[bi];
    if matches!(intent, "chat_about_words" | "just_chatting") {
        return (None, bp);
    }
    let need = if matches!(intent, "delete" | "rename") { THRESHOLD_DESTRUCTIVE } else { THRESHOLD };
    (if bp >= need { Some((intent, bp)) } else { None }, bp)
}

fn narrow(text: &str) -> Vec<&'static str> {
    let t = text.to_lowercase();
    let mut win: Vec<&'static str> = Vec::new();
    for (pat, intents) in FAMILIES {
        if regex::Regex::new(pat).map(|re| re.is_match(&t)).unwrap_or(false) {
            for i in *intents {
                if !win.contains(i) {
                    win.push(i);
                }
            }
        }
    }
    win
}
