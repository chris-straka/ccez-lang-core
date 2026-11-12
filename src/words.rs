//! Words in a line and what they mean, offline: the line cut into
//! words with their readings, and a word's dictionary entry. From
//! ccez-tube's core, itself ported from
//! ccez-lang (`src/core/dictionary.ts`, `offline.ts`,
//! `readings/zhSegment.ts`, `background/japanese.ts`), which builds the
//! dictionary files the app bundles:
//!
//! - Japanese: JMdict (EDRDG), words cut by lindera with IPAdic.
//! - Chinese: CC-CEDICT, words cut by bidirectional maximum matching.
//! - Korean, French, German: English Wiktionary via kaikki.org.
//! - Anything else: words cut on Unicode word breaks, no entries.
//!
//! The app copies or downloads a language's file once and calls
//! [`Words::install`], which packs it ([`crate::dict`]); lookups read
//! the pack from then on. Japanese words and furigana need the `ja`
//! feature (lindera) and IPAdic's files in `root/ipadic`; without them
//! Japanese is cut on Unicode word breaks and still looked up.
#![cfg_attr(not(feature = "ja"), allow(dead_code))]

#[cfg(feature = "ja")]
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[cfg(feature = "ja")]
use lindera::mode::Mode;
#[cfg(feature = "ja")]
use lindera::segmenter::Segmenter;
use serde_json::Value;
use unicode_segmentation::UnicodeSegmentation;

use crate::dict::{self, Pack};
use crate::error::{LangError, Result};
use lexicon::base_lang;

/// One piece of a line. Joining every piece's text gives the line back.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Token {
    pub text: String,
    /// A word to look up (not spaces or punctuation).
    pub word: bool,
    /// The dictionary form when it differs ("食べ" → "食べる").
    pub lemma: Option<String>,
    /// Japanese: the word's kana, in hiragana, when it has kanji;
    /// Chinese: its pinyin.
    pub reading: Option<String>,
    /// The reading laid over the word: furigana over each kanji run
    /// (okurigana bare), pinyin over each Han character. Empty when the
    /// word has none.
    pub ruby: Vec<RubyPart>,
    /// Japanese pitch accent, from Kanjium: the mora the pitch falls
    /// after, 0 when it never falls inside the word. None for a word the
    /// list doesn't have, grammar, and a conjugated form (its accent moves).
    #[cfg_attr(feature = "uniffi", uniffi(default))]
    pub accent: Option<i32>,
}

/// A piece of a word and, when it takes one, the reading over it.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct RubyPart {
    pub text: String,
    pub rt: Option<String>,
    /// Over each mora of the reading (or of the kana, when it has none),
    /// with [`Token::accent`]: 0 low, 1 high, 2 high and the pitch falls
    /// after it. Empty when the word has no accent.
    #[cfg_attr(feature = "uniffi", uniffi(default))]
    pub pitch: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Sense {
    /// Part of speech; for a word with several readings, the reading.
    pub pos: String,
    pub meanings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Entry {
    /// The form the dictionary has (the word as written, or one of its
    /// de-inflected forms).
    pub word: String,
    /// The dictionary form an inflected word points at.
    pub lemma: Option<String>,
    /// Kana for Japanese, pinyin for Chinese.
    pub reading: Option<String>,
    pub ipa: Option<String>,
    /// A noun's gender in words: "feminine".
    pub gender: Option<String>,
    /// Its article where the language has one: "la", "der".
    pub article: Option<String>,
    /// JMdict marks a word common.
    pub common: bool,
    pub senses: Vec<Sense>,
    /// The credit line: "JMdict (EDRDG) · CC BY-SA 4.0".
    pub source: String,
    pub url: String,
}

/// The file name ccez-lang's build gives a language's dictionary, if it
/// makes one (other languages come from the Wiktionary download).
pub fn dictionary_file(lang: &str) -> Option<&'static str> {
    source_file(&base_lang(lang))
}

fn source_file(base: &str) -> Option<&'static str> {
    Some(match base {
        "ja" => "ja.tsv.gz",
        "ko" => "ko.tsv.gz",
        "fr" => "fr.tsv.gz",
        "de" => "de.tsv.gz",
        "es" => "es.tsv.gz",
        "it" => "it.tsv.gz",
        "pt" => "pt.tsv.gz",
        "zh" => "cedict.txt.gz",
        _ => return None,
    })
}

use lexicon::{BY_SA, MAX, candidates, gender_label, unstressed};

pub struct Words {
    root: PathBuf,
    packs: Mutex<HashMap<String, Arc<Pack>>>,
    #[cfg(feature = "ja")]
    japanese: Mutex<Option<Arc<Segmenter>>>,
    pitch: Mutex<Option<Arc<Pack>>>,
}

impl Words {
    fn pitch_path(&self) -> PathBuf {
        self.root.join("ja-pitch.pack")
    }

    fn pitch(&self) -> Option<Arc<Pack>> {
        let mut held = self.pitch.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(p) = held.as_ref() {
            return Some(p.clone());
        }
        let p = Arc::new(Pack::open(&self.pitch_path()).ok()?);
        *held = Some(p.clone());
        Some(p)
    }

    fn pack_path(&self, base: &str) -> PathBuf {
        self.root.join(format!("{base}.pack"))
    }

    fn ipadic(&self) -> PathBuf {
        self.root.join("ipadic")
    }

    fn pack(&self, base: &str) -> Option<Arc<Pack>> {
        let mut packs = self.packs.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(p) = packs.get(base) {
            return Some(p.clone());
        }
        let p = Arc::new(Pack::open(&self.pack_path(base)).ok()?);
        packs.insert(base.to_string(), p.clone());
        Some(p)
    }

    #[cfg(feature = "ja")]
    fn segmenter(&self) -> Option<Arc<Segmenter>> {
        let mut held = self.japanese.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(s) = held.as_ref() {
            return Some(s.clone());
        }
        let dict = lindera::dictionary::load_fs_dictionary(&self.ipadic()).ok()?;
        let s = Arc::new(Segmenter::new(Mode::Normal, dict, None));
        *held = Some(s.clone());
        Some(s)
    }

    #[cfg(not(feature = "ja"))]
    fn japanese(&self, text: &str) -> Vec<Token> {
        plain(text)
    }

    #[cfg(feature = "ja")]
    fn japanese(&self, text: &str) -> Vec<Token> {
        let Some(seg) = self.segmenter() else {
            return plain(text);
        };
        let Ok(mut morphs) = seg.segment(Cow::Borrowed(text)) else {
            return plain(text);
        };
        let pitch = self.pitch();
        let mut out = Vec::new();
        let mut pos = 0;
        for m in morphs.iter_mut() {
            let (start, end) = (m.byte_start, m.byte_end);
            if start < pos || end > text.len() || start >= end {
                continue;
            }
            gap(&mut out, &text[pos..start]);
            pos = end;
            let surface = &text[start..end];
            let d: Vec<String> = m.details().iter().map(|s| s.to_string()).collect();
            let field = |i: usize| d.get(i).filter(|s| s.as_str() != "*" && !s.is_empty());
            let symbol = d.first().is_some_and(|p| p == "記号");
            if symbol || !surface.chars().any(char::is_alphanumeric) {
                gap(&mut out, surface);
                continue;
            }
            let mut t = Token {
                text: surface.to_string(),
                word: true,
                lemma: field(6).filter(|b| b.as_str() != surface).cloned(),
                reading: field(7)
                    .filter(|_| surface.chars().any(is_kanji))
                    .map(|r| to_hiragana(r)),
                ruby: Vec::new(),
                accent: None,
            };
            // Particles and endings ride on the word before.
            let grammar = d.first().is_some_and(|p| matches!(p.as_str(), "助詞" | "助動詞" | "フィラー"));
            if !grammar {
                t.accent = pitch.as_deref().and_then(|p| accent_for(&t, p));
            }
            out.push(t);
        }
        gap(&mut out, &text[pos..]);
        let out = match self.pack("ja") {
            Some(p) => merge_kanji(out, &p),
            None => out,
        };
        out.into_iter()
            .map(|mut t| {
                // A compound joined from kanji words: its own accent.
                if t.accent.is_none() && t.text.chars().all(is_kanji) {
                    t.accent = pitch.as_deref().and_then(|p| accent_for(&t, p));
                }
                let mut ruby = t.reading.as_deref().map(|r| furigana(&t.text, r)).unwrap_or_default();
                if let Some(a) = t.accent {
                    if ruby.is_empty() {
                        ruby = vec![RubyPart { text: t.text.clone(), rt: None, pitch: Vec::new() }];
                    }
                    let kana = t.reading.clone().unwrap_or_else(|| to_hiragana(&t.text));
                    if !mark_pitch(&mut ruby, &kana, a) {
                        // The parts don't line up with the reading: no marks.
                        t.accent = None;
                        if t.reading.is_none() {
                            ruby.clear();
                        }
                    }
                }
                Token { ruby, ..t }
            })
            .collect()
    }

    fn chinese(&self, text: &str) -> Vec<Token> {
        let pack = self.pack("zh");
        let spoken = |w: &str| SPOKEN.iter().find(|(s, _)| *s == w).map(|(_, py)| py.to_string());
        let has = |w: &str| spoken(w).is_some() || pack.as_ref().is_some_and(|p| p.has(w));
        let mut out = Vec::new();
        let mut run = String::new();
        let mut other = String::new();
        let flush_run = |run: &mut String, out: &mut Vec<Token>| {
            for w in segment_han_run(run, &has) {
                let reading = spoken(&w).or_else(|| pack.as_ref().and_then(|p| word_pinyin(p, &w)));
                let ruby = reading.as_deref().map(|r| pinyin_ruby(&w, r)).unwrap_or_default();
                out.push(Token {
                    text: w,
                    word: true,
                    lemma: None,
                    reading,
                    ruby,
                    accent: None,
                });
            }
            run.clear();
        };
        for c in text.chars() {
            if is_han(c) {
                if !other.is_empty() {
                    out.extend(plain(&other));
                    other.clear();
                }
                run.push(c);
            } else {
                flush_run(&mut run, &mut out);
                other.push(c);
            }
        }
        flush_run(&mut run, &mut out);
        out.extend(plain(&other));
        join_gaps(out)
    }

    fn lookup_japanese(&self, p: &Pack, word: &str, lemma: Option<&str>) -> Option<Entry> {
        let mut cands = candidates(word, "ja", lemma);
        let hira = to_hiragana(word);
        if !cands.contains(&hira) {
            cands.push(hira);
        }
        for cand in cands {
            let Some(ids) = p.get(&cand) else { continue };
            let ids: Vec<i64> = serde_json::from_str(&ids).unwrap_or_default();
            let found: Vec<(i64, Value)> = ids
                .into_iter()
                .take(3)
                .filter_map(|id| {
                    let e = p.get(&format!("#{id}"))?;
                    Some((id, serde_json::from_str(&e).ok()?))
                })
                .collect();
            if let Some(e) = ja_entry(&cand, &found) {
                return Some(e);
            }
        }
        None
    }

    fn lookup_wiki(&self, p: &Pack, base: &str, word: &str, lemma: Option<&str>) -> Option<Entry> {
        let cands = candidates(word, base, lemma);
        // A capital at the start of a line finds a name first (Maison, a
        // surname): a form with ordinary senses wins over one with only names.
        let mut name: Option<Entry> = None;
        let mut entry = None;
        for cand in &cands {
            let Some(line) = p.get(cand).and_then(|j| serde_json::from_str::<Value>(&j).ok()) else {
                continue;
            };
            let target = lexicon::form_target(&line);
            let target = target.as_deref();
            // Russian forms name their word with its stress marked
            // (сде́лать); the word itself is filed without.
            let lemma_line = target
                .and_then(|t| {
                    // The plain spelling first: the stress-marked one is
                    // filed only as a pointer to it.
                    let plain = unstressed(t);
                    (plain != t).then(|| p.get(&plain)).flatten().or_else(|| p.get(t))
                })
                .and_then(|j| serde_json::from_str::<Value>(&j).ok());
            let Some(e) = wiki_entry(cand, base, &line, lemma_line.as_ref()) else { continue };
            if e.senses.iter().all(|s| s.pos == "proper noun") {
                name.get_or_insert(e);
                continue;
            }
            entry = Some(e);
            break;
        }
        let mut entry = entry.or(name)?;
        // The sound and a noun's gender: the form found first, then its
        // dictionary form, then the other spellings tried.
        let mut heads: Vec<String> = vec![entry.word.clone()];
        heads.extend(entry.lemma.clone());
        for c in cands.into_iter().take(3) {
            if !heads.contains(&c) {
                heads.push(c);
            }
        }
        for h in &heads {
            let Some(line) = p.get(h).and_then(|j| serde_json::from_str::<Value>(&j).ok()) else {
                continue;
            };
            if entry.ipa.is_none() {
                entry.ipa = line.get(0).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
            }
            // A verb form takes no gender from its base word's noun
            // ("devrai" is not "le devoir").
            if entry.gender.is_none() && !(h != &entry.word && verb_form(&entry)) {
                let g = line.pointer("/1/0/1").and_then(Value::as_str).filter(|s| !s.is_empty());
                if let Some(g) = g {
                    // The dictionary word's gender on its plural: los
                    // zapatos, les maisons, not "el zapatos".
                    let plural_form = h != &entry.word
                        && !g.ends_with("-p")
                        && entry.senses.iter().flat_map(|s| &s.meanings).any(|m| m.starts_with("plural of") || m.contains(" plural of "));
                    let g = if plural_form { format!("{g}-p") } else { g.to_string() };
                    let (label, article) = gender_label(&g, base, &entry.word);
                    entry.gender = Some(label);
                    entry.article = article;
                }
            }
            if entry.ipa.is_some() && entry.gender.is_some() {
                break;
            }
        }
        Some(entry)
    }

    fn lookup_chinese(&self, p: &Pack, word: &str) -> Option<Entry> {
        let lines = |w: &str| -> Vec<Cedict> {
            p.get(w)
                .map(|v| v.split(dict::SEP).filter_map(parse_cedict_line).collect())
                .unwrap_or_default()
        };
        let word = word.trim_matches(|c: char| !c.is_alphanumeric());
        if let Some(e) = cedict_entry(word, &lines(word)) {
            return Some(e);
        }
        if word.chars().count() < 2 {
            return None;
        }
        // Not a word CC-CEDICT knows: its characters one by one.
        let chars: Vec<Entry> = word
            .chars()
            .filter_map(|c| cedict_entry(&c.to_string(), &lines(&c.to_string())))
            .collect();
        let first = chars.first()?;
        Some(Entry {
            word: word.to_string(),
            lemma: None,
            reading: Some(
                chars
                    .iter()
                    .filter_map(|c| c.reading.as_deref()?.split(',').next())
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            ipa: None,
            gender: None,
            article: None,
            common: false,
            senses: chars
                .iter()
                .map(|c| Sense {
                    pos: c.word.clone(),
                    meanings: c.senses.iter().flat_map(|s| s.meanings.clone()).take(MAX).collect(),
                })
                .collect(),
            source: first.source.clone(),
            url: first.url.clone(),
        })
    }
}

impl Words {
    /// `root` holds the packs and the Japanese analyser's files.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            packs: Mutex::new(HashMap::new()),
            #[cfg(feature = "ja")]
            japanese: Mutex::new(None),
            pitch: Mutex::new(None),
        }
    }

    /// Whether the language's dictionary is installed (for Japanese with
    /// the `ja` feature, with its analyser's files in `root/ipadic` and
    /// the pitch accents).
    pub fn ready(&self, lang: &str) -> bool {
        let base = base_lang(lang);
        self.pack_path(&base).exists()
            && (base != "ja" || !cfg!(feature = "ja") || (self.ipadic().join("metadata.json").exists() && self.pitch_path().exists()))
    }

    /// Packs a language's dictionary file at `source` (copied out of the
    /// assets, or downloaded for a language the app doesn't carry) for
    /// lookups. Slow (seconds): call it off the main thread.
    pub fn install(&self, lang: &str, source: &Path) -> Result<()> {
        let base = base_lang(lang);
        let storage = |e: std::io::Error| LangError::Storage { reason: e.to_string() };
        // Any language's Wiktionary file (bundled, or downloaded for a
        // language the app doesn't carry); Chinese is CC-CEDICT.
        let entries = if base == "zh" {
            dict::read_cedict_gz(source)
        } else {
            dict::read_tsv_gz(source)
        }
        .map_err(storage)?;
        std::fs::create_dir_all(&self.root).map_err(storage)?;
        dict::pack(entries, &self.pack_path(&base)).map_err(storage)?;
        self.packs.lock().unwrap_or_else(|p| p.into_inner()).remove(&base);
        Ok(())
    }

    /// Packs Kanjium's Japanese pitch accents (`word<TAB>reading<TAB>accent`,
    /// the reading blank when it's the word) at `source`: by spelling, and
    /// by reading the accent every spelling read so shares, for words
    /// written in kana.
    pub fn install_pitch(&self, source: &Path) -> Result<()> {
        let storage = |e: std::io::Error| LangError::Storage { reason: e.to_string() };
        let rows = dict::read_tsv_gz(source).map_err(storage)?;
        let mut words: HashMap<String, Vec<String>> = HashMap::new();
        let mut readings: HashMap<String, Option<i32>> = HashMap::new();
        for (word, rest) in rows {
            let Some((reading, accent)) = rest.split_once('\t') else { continue };
            let Ok(n) = accent.trim().parse::<i32>() else { continue };
            let reading = if reading.is_empty() { word.as_str() } else { reading };
            words.entry(word.clone()).or_default().push(format!("{reading}\t{n}"));
            readings
                .entry(to_hiragana(reading))
                .and_modify(|had| {
                    if *had != Some(n) {
                        *had = None
                    }
                })
                .or_insert(Some(n));
        }
        let mut entries: Vec<(String, String)> = words.into_iter().map(|(w, v)| (w, v.join(&dict::SEP.to_string()))).collect();
        entries.extend(readings.into_iter().map(|(r, a)| (format!("{BY_READING}{r}"), a.map_or("-".to_string(), |n| n.to_string()))));
        std::fs::create_dir_all(&self.root).map_err(storage)?;
        dict::pack(entries, &self.pitch_path()).map_err(storage)?;
        *self.pitch.lock().unwrap_or_else(|p| p.into_inner()) = None;
        Ok(())
    }

    /// How a Japanese or Chinese line sounds, for telling a different
    /// spelling (街/町, 分かる/わかる) from a different word: each word's
    /// kana in hiragana, or its pinyin. Punctuation and spaces dropped.
    pub fn sound(&self, lang: &str, text: &str) -> String {
        self.tokens(lang, text)
            .into_iter()
            .filter(|t| t.word)
            .map(|t| t.reading.unwrap_or_else(|| to_hiragana(&t.text)).replace(' ', ""))
            .collect()
    }

    /// A line cut into words and what lies between them, each word with
    /// its reading laid over it (furigana, pinyin) where the language has
    /// one. Joining every token's text gives the line back.
    pub fn tokens(&self, lang: &str, text: &str) -> Vec<Token> {
        match base_lang(lang).as_str() {
            "ja" => self.japanese(text),
            "zh" => self.chinese(text),
            _ => plain(text),
        }
    }

    /// A word's entry: its dictionary form first (`lemma`, from
    /// [`Words::tokens`]), then the word as written, then de-inflected
    /// guesses. None when the language has no dictionary installed or
    /// the dictionary doesn't have the word.
    /// The one form a word is saved under, the same in every app: its
    /// dictionary headword in the dictionary's spelling ("freinent" →
    /// "freiner", "l'ardoise" → "ardoise", "wasser" → "Wasser", 食べた →
    /// 食べる with the analyser's `lemma`). A form that is a word of its own
    /// stays itself ("gare", not "garer"); a phrase keeps the dictionary's
    /// form, or its first word's headword and the rest ("découlent de" →
    /// "découler de"). None: nothing to save — a number, a name, a fragment
    /// the dictionary doesn't know. With no dictionary for the language yet,
    /// the text as it is.
    pub fn headword(&self, lang: &str, text: &str, lemma: Option<&str>) -> Option<String> {
        let t = text.trim();
        if t.is_empty() || t.chars().any(|c| c.is_numeric()) {
            return None;
        }
        let base = base_lang(lang);
        if self.pack(&base).is_none() {
            return Some(t.to_string());
        }
        let own = |e: Entry| -> Option<String> {
            if e.senses.iter().all(|s| s.pos == "proper noun") {
                return None;
            }
            // CC-CEDICT names the traditional spelling beside the word: "喜欢 (喜歡)".
            let w = e.lemma.unwrap_or(e.word);
            Some(w.split(" (").next().unwrap_or(&w).to_string())
        };
        if t.contains(char::is_whitespace) {
            if let Some(e) = self.lookup(lang, t, lemma) {
                return own(e);
            }
            let (first, rest) = t.split_once(char::is_whitespace)?;
            let head = self.headword(lang, first, None)?;
            return Some(format!("{head} {}", rest.split_whitespace().collect::<Vec<_>>().join(" ")));
        }
        // A French reflexive verb keeps its "s'" ("s'attarder", not
        // "attarder"; Italian's s' is "si" cut short, and goes).
        if base == "fr" {
            for refl in ["s'", "s’"] {
                if let Some(rest) = t.strip_prefix(refl).filter(|r| !r.is_empty()) {
                    return self.headword(lang, rest, None).map(|h| format!("s'{h}"));
                }
            }
        }
        // A German noun in lower case (some apps lower-case everything):
        // the noun, unless the lower case is a word of its own ("fressen").
        if base == "de" && t.chars().next().is_some_and(char::is_lowercase) {
            let mut cs = t.chars();
            let cap: String = cs.next().map(|c| c.to_uppercase().chain(cs).collect()).unwrap_or_default();
            let noun = self.lookup(lang, &cap, None).filter(|n| n.word == cap && n.lemma.is_none() && n.senses.iter().any(|s| s.pos.starts_with("noun")));
            if noun.is_some() && !self.lookup(lang, t, None).is_some_and(|e| e.word == t && e.lemma.is_none()) {
                return Some(cap);
            }
        }
        own(self.lookup(lang, t, lemma)?)
    }

    pub fn lookup(&self, lang: &str, word: &str, lemma: Option<&str>) -> Option<Entry> {
        let base = base_lang(lang);
        let p = self.pack(&base)?;
        match base.as_str() {
            "ja" => self.lookup_japanese(&p, word, lemma),
            "zh" => self.lookup_chinese(&p, word),
            _ => self.lookup_wiki(&p, &base, word, lemma),
        }
    }
}

// ---- cutting lines ---------------------------------------------------------

/// Unicode word breaks: words, and runs of what lies between them.
fn plain(text: &str) -> Vec<Token> {
    join_gaps(
        text.split_word_bounds()
            .map(|piece| Token {
                text: piece.to_string(),
                word: piece.chars().any(char::is_alphanumeric),
                lemma: None,
                reading: None,
                ruby: Vec::new(),
                accent: None,
            })
            .collect(),
    )
}

/// Adjacent non-words joined into one piece.
fn join_gaps(tokens: Vec<Token>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    for t in tokens {
        if t.text.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last) if !last.word && !t.word => last.text.push_str(&t.text),
            _ => out.push(t),
        }
    }
    out
}

fn gap(out: &mut Vec<Token>, text: &str) {
    if text.is_empty() {
        return;
    }
    match out.last_mut() {
        Some(last) if !last.word => last.text.push_str(text),
        _ => out.push(Token {
            text: text.to_string(),
            word: false,
            lemma: None,
            reading: None,
            ruby: Vec::new(),
            accent: None,
        }),
    }
}

fn is_kanji(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}' | '々')
}

fn is_han(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}' | '\u{20000}'..='\u{2FA1F}')
}

/// Katakana to hiragana; everything else as it is.
pub fn to_hiragana(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{30A1}'..='\u{30F6}' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            _ => c,
        })
        .collect()
}

/// Furigana for a word: the reading over its kanji runs, the kana it
/// shares with the reading at either end left bare (食べる: た over 食).
/// When the kana in the middle can't be matched, the whole word takes
/// the whole reading.
pub fn furigana(surface: &str, reading: &str) -> Vec<RubyPart> {
    let s: Vec<char> = surface.chars().collect();
    let r: Vec<char> = to_hiragana(reading).chars().collect();
    let hira = |c: char| to_hiragana(&c.to_string()).chars().next().unwrap_or(c);
    // Shared kana at the end (okurigana), then at the start.
    let mut tail = 0;
    while tail < s.len() && tail < r.len() && !is_kanji(s[s.len() - 1 - tail]) && hira(s[s.len() - 1 - tail]) == r[r.len() - 1 - tail] {
        tail += 1;
    }
    let mut head = 0;
    while head < s.len() - tail && head < r.len() - tail && !is_kanji(s[head]) && hira(s[head]) == r[head] {
        head += 1;
    }
    let core: String = s[head..s.len() - tail].iter().collect();
    let core_rt: String = r[head..r.len() - tail].iter().collect();
    if core.is_empty() || core_rt.is_empty() || !core.chars().any(is_kanji) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let bare = |cs: &[char]| RubyPart { text: cs.iter().collect(), rt: None, pitch: Vec::new() };
    if head > 0 {
        out.push(bare(&s[..head]));
    }
    out.push(RubyPart { text: core, rt: Some(core_rt), pitch: Vec::new() });
    if tail > 0 {
        out.push(bare(&s[s.len() - tail..]));
    }
    out
}

/// A word's pinyin from CC-CEDICT: an everyday entry (lower-case
/// pinyin) before a name's.
fn word_pinyin(p: &Pack, word: &str) -> Option<String> {
    let entries: Vec<Cedict> = p.get(word)?.split(dict::SEP).filter_map(parse_cedict_line).collect();
    entries
        .iter()
        .find(|e| e.pinyin.chars().next().is_some_and(char::is_lowercase))
        .or(entries.first())
        .map(|e| e.pinyin.clone())
}

/// Pinyin over each Han character when the syllables line up, else
/// over the whole word.
fn pinyin_ruby(word: &str, pinyin: &str) -> Vec<RubyPart> {
    let chars: Vec<char> = word.chars().collect();
    let syllables: Vec<&str> = pinyin.split_whitespace().collect();
    if syllables.len() == chars.len() {
        chars
            .iter()
            .zip(syllables)
            .map(|(c, s)| RubyPart { text: c.to_string(), rt: is_han(*c).then(|| s.to_string()), pitch: Vec::new() })
            .collect()
    } else {
        vec![RubyPart { text: word.to_string(), rt: Some(pinyin.to_string()), pitch: Vec::new() }]
    }
}

/// IPAdic splits some compounds into single kanji (一|歩); a run of them
/// that JMdict has as one word becomes that word.
fn merge_kanji(tokens: Vec<Token>, ja: &Pack) -> Vec<Token> {
    let kanji_only = |t: &Token| t.word && t.text.chars().all(is_kanji);
    let mut out = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let mut end = i;
        while end < tokens.len() && kanji_only(&tokens[end]) && end - i < 6 {
            end += 1;
        }
        let mut merged = false;
        for stop in (i + 2..=end).rev() {
            let text: String = tokens[i..stop].iter().map(|t| t.text.as_str()).collect();
            if ja.has(&text) {
                let reading: Option<String> = tokens[i..stop].iter().map(|t| t.reading.clone()).collect();
                out.push(Token {
                    text,
                    word: true,
                    lemma: None,
                    reading,
                    ruby: Vec::new(),
                    accent: None,
                });
                i = stop;
                merged = true;
                break;
            }
        }
        if !merged {
            out.push(tokens[i].clone());
            i += 1;
        }
    }
    out
}

// ---- pitch accent ----------------------------------------------------------

/// Before a reading's key in the pitch pack (no spelling starts with it).
const BY_READING: char = '\u{1}';

/// Small kana that join the mora before them (きゃ is one mora; っ, ん and ー are their own).
fn joins(c: char) -> bool {
    "ゃゅょぁぃぅぇぉゎャュョァィゥェォヮ".contains(c)
}

/// How many morae a kana string has (きょう: 2).
fn morae(kana: &str) -> usize {
    let mut n = 0;
    for (i, c) in kana.chars().enumerate() {
        if !(i > 0 && joins(c)) {
            n += 1;
        }
    }
    n
}

fn is_kana(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| matches!(c, '\u{3041}'..='\u{3096}' | '\u{30a1}'..='\u{30fa}' | 'ー'))
}

/// A word's accent from the pitch pack (ccez-lang's rules): only the
/// dictionary form (a conjugated word's accent moves, and the list
/// doesn't give it); a word in kana takes its kanji spelling's accent
/// when every spelling read that way agrees (はし: 箸 1, 橋 2, 端 0, none).
fn accent_for(t: &Token, pitch: &Pack) -> Option<i32> {
    if !t.word || t.lemma.is_some() {
        return None;
    }
    let kana = match &t.reading {
        Some(r) => r.clone(),
        None if is_kana(&t.text) => to_hiragana(&t.text),
        None => return None,
    };
    let count = morae(&kana);
    // One kana alone (ね, よ) is grammar riding on the word before.
    if t.reading.is_none() && count < 2 {
        return None;
    }
    let spelled = pitch.get(&t.text).and_then(|v| {
        v.split(dict::SEP)
            .filter_map(|e| e.split_once('\t'))
            .find(|(r, _)| to_hiragana(r) == kana)
            .and_then(|(_, a)| a.parse::<i32>().ok())
    });
    let accent = match spelled {
        Some(a) => a,
        None if t.reading.is_none() => pitch.get(&format!("{BY_READING}{kana}"))?.parse::<i32>().ok()?,
        None => return None,
    };
    (0..=count as i32).contains(&accent).then_some(accent)
}

/// Marks each part's morae with the word's pitch (see [`RubyPart::pitch`]):
/// heiban low then high to the end, atamadaka high first, nakadaka and
/// odaka low, high to the accent, then low. False when the parts'
/// morae don't add up to the reading's.
fn mark_pitch(parts: &mut [RubyPart], kana: &str, accent: i32) -> bool {
    let count = morae(kana);
    let sizes: Vec<usize> = parts.iter().map(|p| morae(&to_hiragana(p.rt.as_deref().unwrap_or(&p.text)))).collect();
    if count == 0 || sizes.iter().sum::<usize>() != count {
        return false;
    }
    let a = accent as usize;
    let high = |i: usize| match a {
        0 => i > 0 || count == 1,
        1 => i == 0,
        _ => i > 0 && i < a,
    };
    let mut at = 0;
    for (p, n) in parts.iter_mut().zip(sizes) {
        p.pitch = (at..at + n).map(|i| if a > 0 && i + 1 == a { 2 } else { i32::from(high(i)) }).collect();
        at += n;
    }
    true
}

/// The longest word the matcher tries (CC-CEDICT idioms rarely run longer).
const MAX_WORD: usize = 8;
/// Everyday spoken words CC-CEDICT leaves out, with their pinyin (a
/// reading per character would give 吧 "bā", the bar).
const SPOKEN: &[(&str, &str)] = &[("好的", "hǎo de"), ("对的", "duì de"), ("好吧", "hǎo ba"), ("好啊", "hǎo a"), ("行吧", "xíng ba")];

/// One run of Han characters as words: forward and backward maximum
/// matching; fewer words wins, then fewer single characters, then the
/// backward pass (天气预报说明天是晴天 reads 说|明天 that way).
fn segment_han_run(run: &str, has: &dyn Fn(&str) -> bool) -> Vec<String> {
    let chars: Vec<char> = run.chars().collect();
    if chars.len() < 2 {
        return if chars.is_empty() { vec![] } else { vec![run.to_string()] };
    }
    let word = |s: &[char]| s.iter().collect::<String>();
    let mut fwd = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let mut n = MAX_WORD.min(chars.len() - i);
        while n > 1 && !has(&word(&chars[i..i + n])) {
            n -= 1;
        }
        fwd.push(word(&chars[i..i + n]));
        i += n;
    }
    let mut back = Vec::new();
    let mut j = chars.len();
    while j > 0 {
        let mut n = MAX_WORD.min(j);
        while n > 1 && !has(&word(&chars[j - n..j])) {
            n -= 1;
        }
        back.push(word(&chars[j - n..j]));
        j -= n;
    }
    back.reverse();
    let singles = |ws: &[String]| ws.iter().filter(|w| w.chars().count() == 1).count();
    if fwd.len() != back.len() {
        return if fwd.len() < back.len() { fwd } else { back };
    }
    if singles(&fwd) < singles(&back) { fwd } else { back }
}

// ---- what to look up --------------------------------------------------------

// ---- entries ------------------------------------------------------------------

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

/// JMdict entries (`[kanji, kana, common, [[pos, glosses]…]]`) as one
/// entry: the best one's senses, then a second's when it reads
/// differently and is common (方: かた and ほう).
fn ja_entry(word: &str, found: &[(i64, Value)]) -> Option<Entry> {
    let (first_id, first) = found.first()?;
    let kana = |e: &Value| strings(e.get(1)).into_iter().next();
    let common = |e: &Value| e.get(2).and_then(Value::as_i64).unwrap_or(0) != 0;
    let mut shown = vec![first];
    if let Some((_, second)) = found.get(1)
        && kana(second) != kana(first)
        && common(second)
    {
        shown.push(second);
    }
    let per = if shown.len() > 1 { 2 } else { MAX };
    let mut senses = Vec::new();
    for e in &shown {
        let Some(list) = e.get(3).and_then(Value::as_array) else { continue };
        for s in list.iter().take(per) {
            let label = strings(s.get(0)).join(", ");
            let pos = match (shown.len() > 1, kana(e)) {
                (true, Some(k)) if label.is_empty() => k,
                (true, Some(k)) => format!("{k} · {label}"),
                _ => label,
            };
            senses.push(Sense {
                pos,
                meanings: strings(s.get(1)).into_iter().take(MAX).collect(),
            });
        }
    }
    senses.truncate(MAX);
    if senses.is_empty() {
        return None;
    }
    let mut readings: Vec<String> = Vec::new();
    for k in shown.iter().filter_map(|e| kana(e)) {
        if !readings.contains(&k) {
            readings.push(k);
        }
    }
    Some(Entry {
        word: word.to_string(),
        lemma: None,
        reading: (!readings.is_empty()).then(|| readings.join(", ")),
        ipa: None,
        gender: None,
        article: None,
        common: common(first),
        senses,
        source: format!("JMdict (EDRDG) · {BY_SA}"),
        url: format!("https://www.edrdg.org/jmwsgi/entr.py?svc=jmdict&q={first_id}"),
    })
}


/// A kaikki line as an entry (ccez-lexicon's, shared with ccez-lang).
fn wiki_entry(word: &str, base: &str, line: &Value, lemma_line: Option<&Value>) -> Option<Entry> {
    let e = lexicon::wiki_entry(word, base, line, lemma_line)?;
    Some(Entry {
        word: e.word,
        lemma: e.lemma,
        reading: None,
        ipa: None,
        gender: None,
        article: None,
        common: false,
        senses: e.senses.into_iter().map(|s| Sense { pos: s.pos, meanings: s.meanings }).collect(),
        source: format!("Wiktionary via kaikki.org · {BY_SA}"),
        url: e.url,
    })
}

struct Cedict {
    trad: String,
    simp: String,
    pinyin: String,
    meanings: Vec<String>,
}

/// `傳統 传统 [chuan2 tong3] /tradition/traditional/`
fn parse_cedict_line(line: &str) -> Option<Cedict> {
    let (trad, rest) = line.split_once(' ')?;
    let (simp, rest) = rest.split_once(' ')?;
    let rest = rest.strip_prefix('[')?;
    let (pinyin, rest) = rest.split_once(']')?;
    let body = rest.trim().strip_prefix('/')?.strip_suffix('/')?;
    Some(Cedict {
        trad: trad.into(),
        simp: simp.into(),
        pinyin: numbered_to_marks(pinyin),
        meanings: body.split('/').filter(|m| !m.is_empty()).map(str::to_string).collect(),
    })
}

/// "zhong1 guo2" → "zhōng guó": a or e takes the mark, else the o of
/// ou, else the last vowel.
pub fn numbered_to_marks(pinyin: &str) -> String {
    const MARKS: [(char, [char; 4]); 6] = [
        ('a', ['ā', 'á', 'ǎ', 'à']),
        ('e', ['ē', 'é', 'ě', 'è']),
        ('i', ['ī', 'í', 'ǐ', 'ì']),
        ('o', ['ō', 'ó', 'ǒ', 'ò']),
        ('u', ['ū', 'ú', 'ǔ', 'ù']),
        ('ü', ['ǖ', 'ǘ', 'ǚ', 'ǜ']),
    ];
    pinyin
        .split(' ')
        .map(|syl| {
            let tone = syl.chars().last().and_then(|c| c.to_digit(10)).filter(|t| (1..=5).contains(t));
            let body = match tone {
                Some(_) => &syl[..syl.len() - 1],
                None => syl,
            };
            let body: Vec<char> = body.replace("u:", "ü").replace('v', "ü").chars().collect();
            let Some(tone) = tone else { return body.iter().collect() };
            if tone == 5 || body.is_empty() {
                return body.iter().collect();
            }
            let lower: Vec<char> = body.iter().flat_map(|c| c.to_lowercase()).collect();
            let at = lower
                .iter()
                .position(|&c| c == 'a' || c == 'e')
                .or_else(|| lower.windows(2).position(|w| w == ['o', 'u']))
                .or_else(|| lower.iter().rposition(|c| "aeiouü".contains(*c)));
            let Some(at) = at else { return body.iter().collect() };
            let v = lower[at];
            let marked = MARKS
                .iter()
                .find(|(base, _)| *base == v)
                .map_or(v, |(_, m)| m[tone as usize - 1]);
            let marked = if body[at] == v { marked } else { marked.to_uppercase().next().unwrap_or(marked) };
            let mut out = body.clone();
            out[at] = marked;
            out.into_iter().collect()
        })
        .collect::<Vec<String>>()
        .join(" ")
}

/// CC-CEDICT entries as one entry; several readings become several senses.
fn cedict_entry(word: &str, found: &[Cedict]) -> Option<Entry> {
    let first = found.first()?;
    let mut readings: Vec<&str> = Vec::new();
    for e in found {
        if !readings.contains(&e.pinyin.as_str()) {
            readings.push(&e.pinyin);
        }
    }
    let senses = found
        .iter()
        .map(|e| Sense {
            pos: if readings.len() > 1 { e.pinyin.clone() } else { String::new() },
            meanings: e.meanings.iter().take(6).cloned().collect(),
        })
        .collect();
    let shown = if first.trad != word && first.simp == word {
        format!("{word} ({})", first.trad)
    } else {
        word.to_string()
    };
    Some(Entry {
        word: shown,
        lemma: None,
        reading: Some(readings.join(", ")),
        ipa: None,
        gender: None,
        article: None,
        common: false,
        senses,
        source: format!("CC-CEDICT · {BY_SA}"),
        url: format!("https://www.mdbg.net/chinese/dictionary?wdqb={word}"),
    })
}

/// The entry is a form of a verb, by what its senses say it is
/// ("first-person singular future of devoir", "past participle of").
fn verb_form(entry: &Entry) -> bool {
    const VERB: [&str; 12] = [
        "-person", "participle", "infinitive", "gerund", "imperative", "subjunctive", "indicative", "conditional",
        "future of", "preterite", "imperfect", "present of",
    ];
    entry.senses.iter().flat_map(|s| &s.meanings).any(|m| VERB.iter().any(|v| m.contains(v)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn texts(tokens: &[Token]) -> Vec<(&str, bool)> {
        tokens.iter().map(|t| (t.text.as_str(), t.word)).collect()
    }

    #[test]
    fn plain_lines_keep_every_character() {
        let line = "« L'homme, dit-il, est là. »";
        let tokens = plain(line);
        assert_eq!(tokens.iter().map(|t| t.text.as_str()).collect::<String>(), line);
        let words: Vec<&str> = tokens.iter().filter(|t| t.word).map(|t| t.text.as_str()).collect();
        assert_eq!(words, ["L'homme", "dit", "il", "est", "là"]);
    }

    #[test]
    fn korean_words_are_eojeol() {
        assert_eq!(texts(&plain("밥을 먹어요.")), [("밥을", true), (" ", false), ("먹어요", true), (".", false)]);
    }

    #[test]
    fn candidates_undo_elision_particles_and_case() {
        assert_eq!(candidates("L'homme,", "fr", None), ["L'homme", "homme", "l'homme"]);
        assert_eq!(candidates("allons", "fr", Some("aller")), ["aller", "allons"]);
        assert!(candidates("밥을", "ko", None).contains(&"밥".to_string()));
        assert!(candidates("먹어요", "ko", None).contains(&"먹다".to_string()));
        assert_eq!(candidates("Häuser", "de", None), ["Häuser", "häuser"]);
        assert_eq!(candidates("schläft", "de", None), ["schläft", "Schläft"]);
    }

    #[test]
    fn han_runs_split_on_the_fewest_words() {
        let words = ["天气", "预报", "说明", "明天", "晴天", "天气预报"];
        let has = |w: &str| words.contains(&w);
        assert_eq!(segment_han_run("天气预报说明天是晴天", &has), ["天气预报", "说", "明天", "是", "晴天"]);
    }

    #[test]
    fn pinyin_takes_its_marks() {
        assert_eq!(numbered_to_marks("zhong1 guo2"), "zhōng guó");
        assert_eq!(numbered_to_marks("lu:4 se4"), "lǜ sè");
        assert_eq!(numbered_to_marks("gou3 r5"), "gǒu r");
        assert_eq!(numbered_to_marks("Xi1 an1"), "Xī ān");
        assert_eq!(numbered_to_marks("dou1"), "dōu");
    }

    #[test]
    fn cedict_lines_read_and_name_their_traditional_form() {
        let e = parse_cedict_line("傳統 传统 [chuan2 tong3] /tradition/traditional/").unwrap();
        let entry = cedict_entry("传统", &[e]).unwrap();
        assert_eq!(entry.word, "传统 (傳統)");
        assert_eq!(entry.reading.as_deref(), Some("chuán tǒng"));
        assert_eq!(entry.senses[0].meanings, ["tradition", "traditional"]);
    }

    #[test]
    fn jmdict_entries_show_a_second_reading_when_common() {
        let kata = json!([["方"], ["かた"], 1, [[["noun"], ["direction", "way"]]]]);
        let hou = json!([["方"], ["ほう"], 1, [[["noun"], ["side"]]]]);
        let e = ja_entry("方", &[(1, kata), (2, hou)]).unwrap();
        assert_eq!(e.reading.as_deref(), Some("かた, ほう"));
        assert_eq!(e.senses[0].pos, "かた · noun");
        assert_eq!(e.senses[1].pos, "ほう · noun");
        assert!(e.common);
    }

    #[test]
    fn kaikki_forms_bring_their_lemma() {
        let allons = json!(["/a.lɔ̃/", [], [["aller", "inflection of aller: first-person plural present indicative"]]]);
        let aller = json!(["/a.le/", [["verb", "", ["to go"]]], []]);
        let e = wiki_entry("allons", "fr", &allons, Some(&aller)).unwrap();
        assert_eq!(e.lemma.as_deref(), Some("aller"));
        assert_eq!(e.senses[0].meanings[0], "inflection of aller: first-person plural present indicative");
        assert_eq!(e.senses[0].meanings[1], "to go");
        assert!(e.url.ends_with("/aller#French"));
    }

    #[test]
    fn genders_come_with_articles() {
        assert_eq!(gender_label("f", "fr", "maison"), ("feminine".into(), Some("la".into())));
        assert_eq!(gender_label("m", "fr", "homme"), ("masculine".into(), Some("un".into())));
        assert_eq!(gender_label("f", "fr", "annonce").1.as_deref(), Some("une"));
        assert_eq!(gender_label("n", "de", "Haus"), ("neuter".into(), Some("das".into())));
        assert_eq!(gender_label("m-p", "de", "Männer"), ("masculine plural".into(), Some("die".into())));
        assert_eq!(gender_label("f", "es", "casa").1.as_deref(), Some("la"));
        assert_eq!(gender_label("m-p", "es", "libros").1.as_deref(), Some("los"));
        assert_eq!(gender_label("m", "pt", "livro").1.as_deref(), Some("o"));
        assert_eq!(gender_label("f-p", "pt", "casas").1.as_deref(), Some("as"));
        assert_eq!(gender_label("m", "it", "libro").1.as_deref(), Some("il"));
        assert_eq!(gender_label("m", "it", "studente").1.as_deref(), Some("lo"));
        assert_eq!(gender_label("m", "it", "amico").1.as_deref(), Some("un"));
        assert_eq!(gender_label("m-p", "it", "amici").1.as_deref(), Some("gli"));
        assert_eq!(gender_label("m-p", "it", "studenti").1.as_deref(), Some("gli"));
        assert_eq!(gender_label("m-p", "it", "zaini").1.as_deref(), Some("gli"));
        assert_eq!(gender_label("m-p", "it", "libri").1.as_deref(), Some("i"));
        assert_eq!(gender_label("f-p", "it", "case").1.as_deref(), Some("le"));
    }

    #[test]
    fn furigana_sits_over_the_kanji_only() {
        let parts = furigana("食べ", "たべ");
        assert_eq!(parts, [RubyPart { text: "食".into(), rt: Some("た".into()), pitch: vec![] }, RubyPart { text: "べ".into(), rt: None, pitch: vec![] }]);
        let parts = furigana("お茶", "おちゃ");
        assert_eq!(parts, [RubyPart { text: "お".into(), rt: None, pitch: vec![] }, RubyPart { text: "茶".into(), rt: Some("ちゃ".into()), pitch: vec![] }]);
        assert_eq!(furigana("音楽", "おんがく"), [RubyPart { text: "音楽".into(), rt: Some("おんがく".into()), pitch: vec![] }]);
        assert!(furigana("ある", "ある").is_empty());
    }

    #[test]
    fn pitch_marks_each_mora() {
        assert_eq!(morae("きょう"), 2);
        assert_eq!(morae("がっこう"), 4);
        assert_eq!(morae("ジャンプ"), 3);
        // Atamadaka: high first, the fall after it.
        let mut parts = furigana("天気", "てんき");
        assert!(mark_pitch(&mut parts, "てんき", 1));
        assert_eq!(parts[0].pitch, [2, 0, 0]);
        // A one-mora heiban word is high (its particle too).
        let mut parts = vec![RubyPart { text: "木".into(), rt: Some("き".into()), pitch: vec![] }];
        assert!(mark_pitch(&mut parts, "き", 0));
        assert_eq!(parts[0].pitch, [1]);
        // Parts that don't add up to the reading: no marks.
        let mut parts = vec![RubyPart { text: "今日".into(), rt: Some("こんにち".into()), pitch: vec![] }];
        assert!(!mark_pitch(&mut parts, "きょう", 1));
    }

    #[test]
    fn pinyin_per_character() {
        assert_eq!(
            pinyin_ruby("北京", "Běi jīng"),
            [RubyPart { text: "北".into(), rt: Some("Běi".into()), pitch: vec![] }, RubyPart { text: "京".into(), rt: Some("jīng".into()), pitch: vec![] }]
        );
    }

    #[test]
    fn hiragana_from_katakana() {
        assert_eq!(to_hiragana("タベル"), "たべる");
        assert_eq!(to_hiragana("コーヒー"), "こーひー");
    }
}
