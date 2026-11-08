//! The word lookup's logic, shared by ccez-tube's core (Android, through
//! UniFFI) and ccez-lang (the browser, as WebAssembly: `wasm` below): the
//! forms of a word to look up, an entry from the offline dictionaries'
//! lines (kaikki.org's Wiktionary, as ccez-lang's trimming writes them),
//! a noun's gender and article. No files, no network: the callers read
//! the lines and hand them in.

use serde_json::{Value, json};

pub const MAX: usize = 4;
pub const BY_SA: &str = "CC BY-SA 4.0";

/// A part of speech and its meanings.
#[derive(Debug, Clone, PartialEq)]
pub struct Sense {
    pub pos: String,
    pub meanings: Vec<String>,
}

/// An entry from a kaikki line: the form found, the dictionary form it
/// points at (when it's only a form), its senses, its Wiktionary page.
#[derive(Debug, Clone, PartialEq)]
pub struct WikiEntry {
    pub word: String,
    pub lemma: Option<String>,
    pub senses: Vec<Sense>,
    pub url: String,
}

pub fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

/// Wiktionary's section name for a language (its page anchor).
pub fn language_name(base: &str) -> &str {
    match base {
        "fr" => "French",
        "de" => "German",
        "ko" => "Korean",
        "es" => "Spanish",
        "it" => "Italian",
        "pt" => "Portuguese",
        "ru" => "Russian",
        "pl" => "Polish",
        "id" => "Indonesian",
        "tr" => "Turkish",
        "no" | "nb" => "Norwegian_Bokmål",
        "cs" => "Czech",
        "el" => "Greek",
        "fa" => "Persian",
        "th" => "Thai",
        "vi" => "Vietnamese",
        "ro" => "Romanian",
        "bg" => "Bulgarian",
        "hu" => "Hungarian",
        "hy" => "Armenian",
        "ar" => "Arabic",
        "hi" => "Hindi",
        "ja" => "Japanese",
        "zh" => "Chinese",
        other => other,
    }
}

const KO_PARTICLES: &[&str] = &[
    "에서는", "에게서", "으로는", "에서", "에게", "한테", "까지", "부터", "으로", "이랑", "하고", "처럼", "보다",
    "마다", "은", "는", "이", "가", "을", "를", "에", "의", "도", "로", "와", "과", "만", "요",
];
const KO_ENDINGS: &[(&str, &str)] = &[
    ("했어요", "하다"),
    ("해요", "하다"),
    ("합니다", "하다"),
    ("했다", "하다"),
    ("하고", "하다"),
    ("았어요", "다"),
    ("었어요", "다"),
    ("어요", "다"),
    ("아요", "다"),
    ("습니다", "다"),
    ("고", "다"),
    ("지만", "다"),
    ("는데", "다"),
];
const FR_ELISIONS: &[&str] = &["l", "d", "j", "m", "n", "s", "t", "c", "qu", "jusqu", "lorsqu", "puisqu"];
const IT_ELISIONS: &[&str] = &[
    "l", "un", "dell", "all", "dall", "nell", "sull", "coll", "quest", "quell", "bell", "sant", "d", "c", "s", "m", "t", "v", "n",
];

/// Forms to try, best first: the dictionary form, the word as written,
/// then de-inflected guesses.
pub fn candidates(word: &str, base: &str, lemma: Option<&str>) -> Vec<String> {
    let w = word.trim_matches(|c: char| !c.is_alphanumeric());
    let mut out: Vec<String> = Vec::new();
    let mut add = |s: String| {
        if !s.is_empty() && !out.contains(&s) {
            out.push(s);
        }
    };
    if let Some(l) = lemma {
        add(l.to_string());
    }
    add(w.to_string());
    match base {
        "es" | "pt" => add(w.to_lowercase()),
        "fr" | "it" => {
            let bare = w
                .find(['\'', '’'])
                .filter(|&at| {
                    let head = w[..at].to_lowercase();
                    if base == "it" { IT_ELISIONS.contains(&head.as_str()) } else { FR_ELISIONS.contains(&head.as_str()) }
                })
                .map(|at| &w[at + w[at..].chars().next().map_or(1, char::len_utf8)..])
                .unwrap_or(w);
            add(bare.to_string());
            add(bare.to_lowercase());
        }
        "ko" => {
            for p in KO_PARTICLES {
                if w.len() > p.len() && w.ends_with(p) {
                    add(w[..w.len() - p.len()].to_string());
                }
            }
            for (end, rep) in KO_ENDINGS {
                if w.len() > end.len() && w.ends_with(end) {
                    add(format!("{}{rep}", &w[..w.len() - end.len()]));
                }
            }
        }
        "de" => {
            add(w.to_lowercase());
            let mut cs = w.chars();
            if let Some(first) = cs.next() {
                add(first.to_uppercase().chain(cs).collect());
            }
        }
        _ => {}
    }
    add(w.to_lowercase());
    out
}

/// A kaikki line (`[ipa, [[pos, gender, glosses]…], [[lemma, note]…]]`)
/// as an entry: the word's own senses, then what it is a form of, with
/// that lemma's meanings ("first-person plural present of aller").
pub fn wiki_entry(word: &str, base: &str, line: &Value, lemma_line: Option<&Value>) -> Option<WikiEntry> {
    let senses_of = |l: &Value| -> Vec<Sense> {
        l.get(1)
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|e| Sense {
                        pos: e.get(0).and_then(Value::as_str).unwrap_or("").to_string(),
                        meanings: strings(e.get(2)).into_iter().take(MAX).collect(),
                    })
                    .collect::<Vec<_>>()
            })
            .map(|mut v| {
                // The letter itself ("a: the first letter…", "name of the
                // letter A") after what the word means in a sentence.
                v.sort_by_key(|s: &Sense| matches!(s.pos.as_str(), "character" | "letter" | "symbol") || s.meanings.iter().all(|m| m.starts_with("Name of the letter") || m.starts_with("The ") && m.contains(" letter of the ")));
                v
            })
            .unwrap_or_default()
    };
    let target = line
        .pointer("/2/0")
        .and_then(|t| Some((t.get(0)?.as_str()?.to_string(), t.get(1)?.as_str()?.to_string())))
        // A form kaikki didn't mark as one: its meaning only names its
        // word ("feminine singular of attaché"), which is no help alone.
        .or_else(|| {
            senses_of(line).iter().flat_map(|s| &s.meanings).find_map(|m| Some((only_points_at(m)?, m.clone())))
        });
    // Meanings that only point at the target: the note says as much.
    let own: Vec<Sense> = senses_of(line)
        .into_iter()
        .map(|mut s| {
            s.meanings.retain(|m| target.as_ref().is_none_or(|(t, _)| only_points_at(m).as_ref() != Some(t)));
            s
        })
        .filter(|s| !s.meanings.is_empty())
        .collect();
    let mut senses = own.clone();
    if let Some((_, note)) = &target {
        let lemma_senses = lemma_line.map(senses_of).unwrap_or_default();
        match lemma_senses.first() {
            Some(first) => senses.push(Sense {
                pos: first.pos.clone(),
                meanings: std::iter::once(note.clone())
                    .chain(first.meanings.iter().cloned())
                    .take(MAX)
                    .collect(),
            }),
            None => senses.push(Sense {
                pos: String::new(),
                meanings: vec![note.clone()],
            }),
        }
        senses.extend(lemma_senses.into_iter().skip(1).take(1));
    }
    senses.truncate(MAX);
    if senses.is_empty() {
        return None;
    }
    let lemma = target.filter(|_| own.is_empty()).map(|(l, _)| l);
    let page = lemma.as_deref().unwrap_or(word);
    Some(WikiEntry {
        word: word.to_string(),
        url: format!("https://en.wiktionary.org/wiki/{page}#{}", language_name(base)),
        lemma,
        senses,
    })
}

/// An Italian masculine word that takes lo (and gli): s + consonant, z,
/// gn, ps, x, y (lo studente, gli zaini).
fn lo_word(word: &str) -> bool {
    let w = word.to_lowercase();
    w.starts_with('z')
        || w.starts_with("gn")
        || w.starts_with("ps")
        || w.starts_with('x')
        || w.starts_with('y')
        || (w.starts_with('s') && !w.chars().nth(1).is_some_and(|c| "aeiou".contains(c)))
}

/// A word without the stress and tone marks dictionaries add to it
/// (сде́лать → сделать).
pub fn unstressed(word: &str) -> String {
    word.chars().filter(|c| !matches!(c, '\u{0301}' | '\u{0300}')).collect()
}

/// "feminine" and, where the language has one, its article ("la", "l’").
pub fn gender_label(gender: &str, base: &str, word: &str) -> (String, Option<String>) {
    let (g, plural) = match gender.strip_suffix("-p") {
        Some(g) => (g, true),
        None => (gender, false),
    };
    let name = match g {
        "m" => "masculine",
        "f" => "feminine",
        "n" => "neuter",
        "c" => "common",
        "mf" => "masculine or feminine",
        "p" => "plural",
        other => other,
    };
    let label = if plural { format!("{name} plural") } else { name.to_string() };
    let key = if plural { "p" } else { g };
    let first = word.chars().next().map(|c| c.to_lowercase().next().unwrap_or(c));
    let vowel = first.is_some_and(|c| "aeiouhâàáéèêëíîïóôúûùœ".contains(c));
    let article = match (base, key, g) {
        ("de", "m", _) => Some("der"),
        ("de", "f" | "p", _) => Some("die"),
        ("de", "n", _) => Some("das"),
        ("fr", "m", _) => Some("le"),
        ("fr", "f", _) => Some("la"),
        ("fr", "p", _) => Some("les"),
        ("es", "m", _) => Some("el"),
        ("es", "f", _) => Some("la"),
        ("es", "p", "m") => Some("los"),
        ("es", "p", "f") => Some("las"),
        ("pt", "m", _) => Some("o"),
        ("pt", "f", _) => Some("a"),
        ("pt", "p", "m") => Some("os"),
        ("pt", "p", "f") => Some("as"),
        // lo (gli) before s + consonant, z, gn, ps, x, y.
        ("it", "m", _) if lo_word(word) => Some("lo"),
        ("it", "m", _) => Some("il"),
        ("it", "f", _) => Some("la"),
        ("it", "p", "f") => Some("le"),
        ("it", "p", "m") => Some(if vowel || lo_word(word) { "gli" } else { "i" }),
        _ => None,
    };
    // Where the article would elide (l'homme, l'annonce), the indefinite
    // one instead, which still says the gender (owner: "une annonce",
    // never "l'annonce").
    let elides = !plural && vowel && matches!((base, g), ("fr", _) | ("it", "m" | "f"));
    let article = if elides {
        match (base, g) {
            ("fr", "f") => Some("une"),
            ("fr", _) => Some("un"),
            ("it", "f") => Some("un’"),
            _ => Some("un"),
        }
    } else {
        article
    };
    (label, article.map(str::to_string))
}

/// A language tag's primary subtag, lowercased, with the old aliases
/// folded ("fr-FR" → "fr", "iw" → "he", "nb" → "no").
pub fn base_lang(tag: &str) -> String {
    let base = tag.split(['-', '_', '.']).next().unwrap_or("").to_ascii_lowercase();
    match base.as_str() {
        "iw" => "he".into(),
        "nb" | "nn" => "no".into(),
        "in" => "id".into(),
        "ji" => "yi".into(),
        _ => base,
    }
}

/// The last word before "of" in a meaning that only points at another
/// word: "plural of", "past participle of", "alternative form of".
const POINTERS: &[&str] = &[
    "form", "forms", "spelling", "plural", "singular", "dual", "participle", "inflection",
    "gerund", "infinitive", "imperative", "indicative", "subjunctive", "conditional",
    "preterite", "imperfect", "present", "past", "future", "perfect", "feminine",
    "masculine", "neuter", "diminutive", "augmentative", "superlative", "comparative",
    "synonym", "abbreviation", "contraction", "acronym", "initialism", "clipping",
    "misspelling", "variant", "genitive", "dative", "accusative", "nominative",
    "vocative", "ablative", "instrumental", "locative", "romanization", "ellipsis",
    "noun", "adjective", "adverb", "verb", "apocopic", "apheretic", "elative",
];

/// The words a meaning sends the learner to ("plural of chat" → chat,
/// "Short for sympathique" → sympathique, "see aller"), in the word's own
/// language, so each can be looked up in turn. A meaning that only
/// explains in English points nowhere.
pub fn references(meaning: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    // Each clause on its own: "feminine of beau; plural of bel".
    for clause in meaning.split([';', '(', ')']) {
        let words: Vec<&str> = clause.split_whitespace().collect();
        for (i, w) in words.iter().enumerate() {
            let lower = w.to_lowercase();
            let points = match lower.as_str() {
                "of" => i > 0 && POINTERS.contains(&words[i - 1].to_lowercase().trim_matches(',')),
                "for" => i > 0 && matches!(words[i - 1].to_lowercase().as_str(), "short" | "abbreviation" | "contraction"),
                "see" | "cf." | "compare" => i == 0,
                _ => false,
            };
            if !points {
                continue;
            }
            // The word after, up to a colon or comma: up to three words
            // ("à la carte"), never a whole explanation.
            let rest = words[i + 1..].join(" ");
            let target = rest.split([':', ',', '"', '“', '”']).next().unwrap_or("").trim();
            let target = target.trim_end_matches(['.', '!', '?']).trim();
            let n = target.split_whitespace().count();
            if n == 0 || n > 3 || out.iter().any(|o| o == target) {
                continue;
            }
            out.push(target.to_string());
        }
    }
    out
}

/// The word a meaning is nothing but a pointer to: "feminine singular
/// of attaché" → attaché, "inflection of aller:" → aller.
pub fn only_points_at(meaning: &str) -> Option<String> {
    let refs = references(meaning);
    let [r] = refs.as_slice() else { return None };
    let m = meaning.trim().trim_end_matches([':', '.', ' ']);
    let before = m.strip_suffix(r.as_str())?;
    (before.split_whitespace().count() <= 7).then(|| r.clone())
}

/// The dictionary word a kaikki line is a form of: the one it names
/// (`/2/0/0`), else the one a meaning only points at.
pub fn form_target(line: &Value) -> Option<String> {
    if let Some(t) = line.pointer("/2/0/0").and_then(Value::as_str) {
        return Some(t.to_string());
    }
    line.get(1)?.as_array()?.iter().flat_map(|e| strings(e.get(2))).find_map(|m| only_points_at(&m))
}

/// One request, as JSON in and JSON out (how ccez-lang calls it through
/// WebAssembly): `{"fn": "candidates", "word", "lang", "lemma"?}`,
/// `{"fn": "entry", "word", "lang", "line", "lemmaLine"?}` (the lines as
/// the dictionary has them, JSON text or values), `{"fn": "gender",
/// "gender", "lang", "word"}`, `{"fn": "unstressed", "word"}`,
/// `{"fn": "references", "meaning"}`, `{"fn": "target", "line"}` (the
/// word to fetch as `lemmaLine`). A bad
/// request answers `null`.
pub fn call(request: &str) -> String {
    let Ok(req) = serde_json::from_str::<Value>(request) else { return "null".into() };
    let s = |k: &str| req.get(k).and_then(Value::as_str);
    let lang = base_lang(s("lang").unwrap_or(""));
    let answer = match s("fn") {
        Some("candidates") => json!(candidates(s("word").unwrap_or(""), &lang, s("lemma"))),
        Some("references") => json!(references(s("meaning").unwrap_or(""))),
        Some("target") => match req.get("line") {
            Some(Value::String(t)) => json!(serde_json::from_str::<Value>(t).ok().as_ref().and_then(form_target)),
            Some(v) => json!(form_target(v)),
            None => Value::Null,
        },
        Some("entry") => {
            // A line comes as its JSON text (as stored) or already parsed.
            let parse = |k: &str| match req.get(k) {
                Some(Value::String(t)) => serde_json::from_str::<Value>(t).ok(),
                Some(Value::Null) | None => None,
                Some(v) => Some(v.clone()),
            };
            match parse("line") {
                Some(line) => match wiki_entry(s("word").unwrap_or(""), &lang, &line, parse("lemmaLine").as_ref()) {
                    Some(e) => json!({
                        "word": e.word,
                        "lemma": e.lemma,
                        "senses": e.senses.iter().map(|x| json!({ "pos": x.pos, "meanings": x.meanings })).collect::<Vec<_>>(),
                        "url": e.url,
                    }),
                    None => Value::Null,
                },
                None => Value::Null,
            }
        }
        Some("gender") => {
            let (label, article) = gender_label(s("gender").unwrap_or(""), &lang, s("word").unwrap_or(""));
            json!({ "label": label, "article": article })
        }
        Some("unstressed") => json!(unstressed(s("word").unwrap_or(""))),
        _ => Value::Null,
    };
    answer.to_string()
}

/// The WebAssembly face: the caller writes a UTF-8 request into memory
/// from `alloc`, calls `call` with its place and length, reads the answer
/// at the place and length packed into the result (high and low 32 bits),
/// and gives both back with `dealloc`.
#[cfg(target_arch = "wasm32")]
mod wasm {
    #[unsafe(no_mangle)]
    pub extern "C" fn alloc(len: usize) -> *mut u8 {
        let mut buf = Vec::<u8>::with_capacity(len.max(1));
        let ptr = buf.as_mut_ptr();
        std::mem::forget(buf);
        ptr
    }

    /// # Safety
    /// `ptr` and `len` must be a block `alloc` gave (or `call` answered with).
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
        drop(unsafe { Vec::from_raw_parts(ptr, 0, len.max(1)) });
    }

    /// # Safety
    /// `ptr` and `len` must be a UTF-8 request written into a block from `alloc`.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn call(ptr: *const u8, len: usize) -> u64 {
        let request = unsafe { std::str::from_utf8(std::slice::from_raw_parts(ptr, len)) }.unwrap_or("");
        let mut out = super::call(request).into_bytes();
        out.shrink_to_fit();
        let (p, n) = (out.as_mut_ptr(), out.len());
        std::mem::forget(out);
        ((p as u64) << 32) | n as u64
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_form_brings_its_words_meanings() {
        let attachee = serde_json::json!(["", [["noun", "f", ["feminine singular of attaché"]]], []]);
        let attache = serde_json::json!(["", [["noun", "m", ["attaché (diplomatic official)", "assistant"]]], []]);
        assert_eq!(super::form_target(&attachee).as_deref(), Some("attaché"));
        let e = super::wiki_entry("attachée", "fr", &attachee, Some(&attache)).unwrap();
        assert_eq!(e.lemma.as_deref(), Some("attaché"));
        assert_eq!(e.senses.len(), 1);
        assert_eq!(e.senses[0].meanings, ["feminine singular of attaché", "attaché (diplomatic official)", "assistant"]);
        // A word with meanings of its own keeps them first.
        let both = serde_json::json!(["", [["adj", "", ["tied up"]], ["adj", "", ["feminine singular of attaché"]]], []]);
        let e = super::wiki_entry("attachée", "fr", &both, Some(&attache)).unwrap();
        assert_eq!(e.senses[0].meanings, ["tied up"]);
        assert_eq!(e.senses[1].meanings[0], "feminine singular of attaché");
        assert!(e.lemma.is_none());
    }

    #[test]
    fn meanings_point_at_words() {
        let r = |m: &str| super::references(m);
        assert_eq!(r("plural of chat"), ["chat"]);
        assert_eq!(r("first-person plural present indicative of aller"), ["aller"]);
        assert_eq!(r("inflection of schön:"), ["schön"]);
        assert_eq!(r("feminine singular of beau; plural of bel"), ["beau", "bel"]);
        assert_eq!(r("Alternative form of à la carte."), ["à la carte"]);
        assert_eq!(r("Short for sympathique"), ["sympathique"]);
        assert_eq!(r("past participle of hacer (“to do”)"), ["hacer"]);
        assert!(r("a piece of bread").is_empty());
        assert!(r("the state of being tired").is_empty());
        assert!(r("the leader of a group of people who travel together").is_empty());
    }

    use super::*;

    #[test]
    fn requests_answer_as_json() {
        assert_eq!(call(r#"{"fn":"candidates","word":"L'École","lang":"fr-FR"}"#), r#"["L'École","École","école","l'école"]"#);
        assert_eq!(call(r#"{"fn":"gender","gender":"m","lang":"it","word":"studente"}"#), r#"{"article":"lo","label":"masculine"}"#);
        assert_eq!(call(r#"{"fn":"unstressed","word":"сде́лать"}"#), r#""сделать""#);
        let e: Value = serde_json::from_str(&call(
            r#"{"fn":"entry","word":"voy","lang":"es","line":"[\"\",[],[[\"ir\",\"first-person singular present indicative of ir\"]]]","lemmaLine":"[\"\",[[\"verb\",\"\",[\"to go\"]]],[]]"}"#,
        ))
        .unwrap();
        assert_eq!(e["lemma"], "ir");
        assert_eq!(e["senses"][0]["meanings"][0], "first-person singular present indicative of ir");
        assert_eq!(e["senses"][0]["meanings"][1], "to go");
        assert_eq!(e["url"], "https://en.wiktionary.org/wiki/ir#Spanish");
        assert_eq!(call("not json"), "null");
        assert_eq!(call(r#"{"fn":"nope"}"#), "null");
    }
}
