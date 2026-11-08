//! What the learner knows, one standard for every app: a word is
//! Learning (saved, not known yet), Known, or New (anything else, never
//! stored). Kept here per language and synced with ccez-words, the
//! owner's server, which every app and device shares: newest change
//! wins per word, whatever app made it.
//!
//! One file per language in `dir`, `status-<lang>.tsv`: the word's key
//! (see [`key`]), the word as written, its status. `sync.json` holds
//! where the last sync left off.
//!
//! The server (`server` in [`Statuses::sync`]) is the app's setting,
//! never written here: `GET /words?since=<seq>` answers
//! `{words: [{lang, word, status, at}], seq}`; `POST /words` takes
//! `[{lang, word, status, at, from}]`. A change that loses to a newer
//! one comes back down on the next round.

#![cfg_attr(not(feature = "net"), allow(dead_code))]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
#[cfg(feature = "net")]
use std::sync::atomic::Ordering;

use lexicon::base_lang;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum Status {
    New,
    Learning,
    Known,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::New => "new",
            Status::Learning => "learning",
            Status::Known => "known",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "new" => Status::New,
            "learning" => Status::Learning,
            "known" => Status::Known,
            _ => return None,
        })
    }
}

/// A word's status as it came from another app.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct WordState {
    pub lang: String,
    pub word: String,
    pub status: Status,
}

/// The form a word is kept under, as ccez-words keys it: one Unicode
/// form, lower case, spaces and punctuation trimmed from the edges (not
/// every non-letter: Thai's ไม่ and ไม้ are two words).
pub fn key(word: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    let edge = |c: char| c.is_whitespace() || c.is_ascii_punctuation() || "“”‘’«»„‚‹›…–—、。，．！？：；（）「」『』【】〈〉《》·¿¡".contains(c);
    word.nfc().collect::<String>().trim_matches(edge).to_lowercase()
}

/// key → (the word as written, status).
type Lang = BTreeMap<String, (String, Status)>;

pub struct Statuses {
    dir: PathBuf,
    langs: Mutex<HashMap<String, Lang>>,
    #[cfg_attr(not(feature = "net"), allow(dead_code))]
    syncing: AtomicBool,
}

impl Statuses {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into(), langs: Mutex::new(HashMap::new()), syncing: AtomicBool::new(false) }
    }

    fn path(&self, lang: &str) -> PathBuf {
        self.dir.join(format!("status-{lang}.tsv"))
    }

    fn with<T>(&self, lang: &str, f: impl FnOnce(&mut Lang) -> T) -> T {
        let mut langs = self.langs.lock().unwrap_or_else(|p| p.into_inner());
        let words = langs.entry(lang.to_string()).or_insert_with(|| {
            std::fs::read_to_string(self.path(lang))
                .unwrap_or_default()
                .lines()
                .filter_map(|l| {
                    let mut f = l.split('\t');
                    let k = f.next()?.to_string();
                    let word = f.next()?.to_string();
                    let status = Status::parse(f.next()?)?;
                    (status != Status::New).then_some((k, (word, status)))
                })
                .collect()
        });
        f(words)
    }

    fn save(&self, lang: &str) {
        let path = self.path(lang);
        let dir = self.dir.clone();
        self.with(lang, |words| {
            let body: String = words.iter().map(|(k, (w, s))| format!("{k}\t{w}\t{}\n", s.as_str())).collect();
            let _ = std::fs::create_dir_all(&dir);
            write_safely(&path, &body);
        });
    }

    fn langs_on_disk(&self) -> Vec<String> {
        let mut out: HashSet<String> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|f| f.file_name().to_string_lossy().strip_prefix("status-").and_then(|n| n.strip_suffix(".tsv")).map(str::to_string))
            .collect();
        out.extend(self.langs.lock().unwrap_or_else(|p| p.into_inner()).keys().cloned());
        out.into_iter().collect()
    }

    /// A word's status (New when it was never saved or known).
    pub fn status(&self, lang: &str, word: &str) -> Status {
        let lang = base_lang(lang);
        self.with(&lang, |w| w.get(&key(word)).map_or(Status::New, |(_, s)| *s))
    }

    /// Sets a word's status; New forgets it.
    pub fn set(&self, lang: &str, word: &str, status: Status) {
        let lang = base_lang(lang);
        let k = key(word);
        if k.is_empty() {
            return;
        }
        self.with(&lang, |w| match status {
            Status::New => {
                w.remove(&k);
            }
            s => {
                w.insert(k, (word.trim().to_string(), s));
            }
        });
        self.save(&lang);
    }

    /// The words with this status, as written.
    pub fn words(&self, lang: &str, status: Status) -> Vec<String> {
        let lang = base_lang(lang);
        self.with(&lang, |w| w.values().filter(|(_, s)| *s == status).map(|(w, _)| w.clone()).collect())
    }

    /// The share (0-1) of `words`' distinct words that are known, None
    /// when there are none.
    pub fn known_share(&self, lang: &str, words: &[String]) -> Option<f64> {
        let lang = base_lang(lang);
        let distinct: HashSet<String> = words.iter().map(|w| key(w)).filter(|w| !w.is_empty()).collect();
        if distinct.is_empty() {
            return None;
        }
        let known = self.with(&lang, |w| distinct.iter().filter(|k| w.get(*k).is_some_and(|(_, s)| *s == Status::Known)).count());
        Some(known as f64 / distinct.len() as f64)
    }

    /// Every word saved or known, key "lang:key" → (lang, as written, status).
    fn snapshot(&self) -> BTreeMap<String, (String, String, Status)> {
        let mut now = BTreeMap::new();
        for lang in self.langs_on_disk() {
            self.with(&lang, |w| {
                for (k, (word, s)) in w.iter() {
                    now.insert(format!("{lang}:{k}"), (lang.clone(), word.clone(), *s));
                }
            });
        }
        now
    }
}

/// Where the last sync left off: the server's count, each word's status
/// as last sent or taken in, and the changes made here not yet sent,
/// each with when it was first seen (an offline change keeps its own
/// time, not the time it finally goes up).
#[derive(Default)]
struct SyncState {
    cursor: i64,
    sent: BTreeMap<String, Status>,
    pending: BTreeMap<String, (Status, i64)>,
    /// No sync.json yet (a fresh install, cleared data): what's here has no known time.
    fresh: bool,
}

impl SyncState {
    fn read(path: &Path) -> Self {
        let Some(v) = std::fs::read(path).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) else {
            return Self { fresh: true, ..Self::default() };
        };
        let sent = v["sent"].as_object().into_iter().flatten().filter_map(|(k, s)| Some((k.clone(), Status::parse(s.as_str()?)?))).collect();
        let pending = v["pending"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(k, p)| Some((k.clone(), (Status::parse(p["status"].as_str()?)?, p["at"].as_i64()?))))
            .collect();
        Self { cursor: v["cursor"].as_i64().unwrap_or(0), sent, pending, fresh: false }
    }

    fn json(&self) -> String {
        let sent: serde_json::Map<String, serde_json::Value> = self.sent.iter().map(|(k, s)| (k.clone(), s.as_str().into())).collect();
        let pending: serde_json::Map<String, serde_json::Value> =
            self.pending.iter().map(|(k, (st, at))| (k.clone(), serde_json::json!({ "status": st.as_str(), "at": at }))).collect();
        serde_json::json!({ "cursor": self.cursor, "sent": sent, "pending": pending }).to_string()
    }

    /// Notes what differs from what was last sent, stamped `at` the first
    /// time it's seen; what went back to its sent status is dropped.
    fn note(&mut self, now: &BTreeMap<String, (String, String, Status)>, at: i64) {
        let mut want: BTreeMap<String, Status> =
            now.iter().filter(|(k, (_, _, st))| self.sent.get(*k) != Some(st)).map(|(k, (_, _, st))| (k.clone(), *st)).collect();
        for (k, s) in &self.sent {
            if !now.contains_key(k) && *s != Status::New {
                want.insert(k.clone(), Status::New);
            }
        }
        self.pending.retain(|k, (st, _)| want.get(k) == Some(st));
        for (k, st) in want {
            self.pending.entry(k).or_insert((st, at));
        }
    }
}

#[cfg(feature = "net")]
impl Statuses {
    /// One round with ccez-words at `server`: what changed elsewhere
    /// comes down first and is taken in (newest wins, so a later change
    /// here stays), then what changed here goes up. Returns what came
    /// down, for the app's own lists. `from` names this app and device
    /// ("ccez-keys:phone"). A round already running makes this a no-op.
    pub async fn sync(&self, server: &str, from: &str) -> crate::Result<Vec<WordState>> {
        use crate::LangError;
        if self.syncing.swap(true, Ordering::SeqCst) {
            return Ok(Vec::new());
        }
        struct Done<'a>(&'a AtomicBool);
        impl Drop for Done<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::SeqCst);
            }
        }
        let _done = Done(&self.syncing);

        let state_path = self.dir.join("sync.json");
        let mut state = SyncState::read(&state_path);
        let clock = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
        let now = self.snapshot();
        // Noted before any network: offline, a change keeps the time it
        // was made. A fresh install's words have no time, so anything
        // another app changed beats them.
        state.note(&now, if state.fresh { 1 } else { clock });
        let _ = std::fs::create_dir_all(&self.dir);
        write_safely(&state_path, &state.json());
        state.fresh = false;

        let http = reqwest::Client::builder().timeout(std::time::Duration::from_secs(20)).build().unwrap_or_default();
        let base = server.trim_end_matches('/');
        let net = |e: reqwest::Error| if e.is_timeout() { LangError::Timeout } else { LangError::Network };
        let res = http.get(format!("{base}/words?since={}", state.cursor)).send().await.map_err(net)?;
        let down: serde_json::Value = serde_json::from_str(&res.text().await.map_err(net)?).map_err(|_| LangError::BadResponse)?;
        let mut came = Vec::new();
        for w in down["words"].as_array().into_iter().flatten() {
            let (Some(lang), Some(word), Some(status)) = (w["lang"].as_str(), w["word"].as_str(), w["status"].as_str().and_then(Status::parse)) else {
                continue;
            };
            let at = w["at"].as_i64().unwrap_or(0);
            let lang = base_lang(lang);
            let k = format!("{lang}:{}", key(word));
            // Changed here later than there: ours goes up instead.
            if state.pending.get(&k).is_some_and(|(_, mine)| *mine > at) {
                continue;
            }
            state.pending.remove(&k);
            state.sent.insert(k.clone(), status);
            let here = now.get(&k).map(|(_, _, s)| *s);
            if here == Some(status) || (status == Status::New && here.is_none()) {
                continue;
            }
            self.with(&lang, |words| match status {
                Status::New => {
                    words.remove(&key(word));
                }
                s => {
                    words.insert(key(word), (word.to_string(), s));
                }
            });
            self.save(&lang);
            came.push(WordState { lang, word: word.to_string(), status });
        }
        state.cursor = down["seq"].as_i64().unwrap_or(state.cursor);
        write_safely(&state_path, &state.json());

        if !state.pending.is_empty() {
            let up: Vec<serde_json::Value> = state
                .pending
                .iter()
                .filter_map(|(k, (st, at))| {
                    let (lang, w) = k.split_once(':')?;
                    let word = now.get(k).map_or(w, |(_, written, _)| written.as_str());
                    Some(serde_json::json!({ "lang": lang, "word": word, "status": st.as_str(), "at": at, "from": from }))
                })
                .collect();
            let posted = http
                .post(format!("{base}/words"))
                .header("content-type", "application/json")
                .body(serde_json::Value::Array(up).to_string())
                .send()
                .await;
            // Not taken: it stays pending and goes up next round.
            if posted.is_ok_and(|res| res.status().is_success()) {
                for (k, (st, _)) in std::mem::take(&mut state.pending) {
                    state.sent.insert(k, st);
                }
                write_safely(&state_path, &state.json());
            }
        }
        Ok(came)
    }
}

/// Writes `body` to `path` whole or not at all.
fn write_safely(path: &Path, body: &str) {
    let tmp = path.with_extension("part");
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(tmp, path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> (Statuses, PathBuf) {
        let dir = std::env::temp_dir().join(format!("ccez-status-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        (Statuses::new(&dir), dir)
    }

    #[test]
    fn keys_keep_marks_and_one_unicode_form() {
        assert_ne!(key("ไม่"), key("ไม้"));
        assert_eq!(key("e\u{301}te\u{301}"), key("été"));
        assert_eq!(key("« Maison, »"), "maison");
        assert_eq!(key("「食べる」"), "食べる");
    }

    #[test]
    fn statuses_are_kept_and_new_forgets() {
        let (s, dir) = store("keep");
        assert_eq!(s.status("fr", "maison"), Status::New);
        s.set("fr-FR", "Maison", Status::Learning);
        s.set("ja", "猫", Status::Known);
        assert_eq!(s.status("fr", "maison,"), Status::Learning);
        let again = Statuses::new(&dir);
        assert_eq!(again.status("ja", "猫"), Status::Known);
        assert_eq!(again.words("fr", Status::Learning), ["Maison"]);
        again.set("fr", "maison", Status::New);
        assert_eq!(Statuses::new(&dir).status("fr", "maison"), Status::New);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn known_share_counts_distinct_words() {
        let (s, dir) = store("share");
        s.set("zh", "猫", Status::Known);
        let w: Vec<String> = ["猫", "狗", "猫"].iter().map(|x| x.to_string()).collect();
        assert_eq!(s.known_share("zh", &w), Some(0.5));
        assert_eq!(s.known_share("zh", &[]), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
