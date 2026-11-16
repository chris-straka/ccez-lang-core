//! The learner's AI: one small client for an OpenAI-style
//! `chat/completions` server, streamed, and the prompts the apps share.
//! DeepSeek by default (owner, 2026-11-16); the server's address and
//! model are settings ([`Ai`]), so another model is a settings change.
//! The prompts are ccez-llm's annotation prompts (`src/lib/reading.ts`) as
//! ccez-tube ported them, plus the line explanation from ccez-lang
//! (`src/core/ai/prompts.ts`).

/// DeepSeek's API (api-docs.deepseek.com), the default server.
pub const DEFAULT_URL: &str = "https://api.deepseek.com";
/// DeepSeek V4.1 Flash. `deepseek-v4-flash` is a retired alias.
pub const DEFAULT_MODEL: &str = "deepseek-flash";

/// Where the AI is and the key for it. The key comes in with each call
/// and is never logged or kept here. An empty key sends no
/// Authorization: a hub that adds the owner's key (ccez-words) needs none.
#[derive(Debug, Clone, PartialEq)]
pub struct Ai {
    pub url: String,
    pub model: String,
    pub key: String,
}

impl Ai {
    /// DeepSeek with `key`; a blank url or model takes the default.
    pub fn new(url: &str, model: &str, key: &str) -> Self {
        let pick = |v: &str, d: &str| if v.trim().is_empty() { d.to_string() } else { v.trim().to_string() };
        Ai { url: pick(url, DEFAULT_URL).trim_end_matches('/').to_string(), model: pick(model, DEFAULT_MODEL), key: key.trim().to_string() }
    }
}

/// What a call is for, which sets how hard the model thinks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum Mode {
    /// Corrections, glosses, tips, translation: thinking off, no tools,
    /// so the answer comes at once.
    Quick,
    /// An open chat: light thinking (DeepSeek's lowest effort) and
    /// DeepSeek's own web search and link reading, which it runs itself.
    Ask,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Message {
    pub role: String,
    pub content: String,
    /// An earlier answer's own fields to send back with it (its thinking
    /// and the web lookups, JSON from [`Reply::carry`]); None otherwise.
    pub carry: Option<String>,
}

/// One earlier exchange in a thread: what the learner wrote, the answer.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Turn {
    pub question: String,
    pub answer: String,
}

pub(crate) fn msg(role: &str, content: String) -> Message {
    Message { role: role.into(), content, carry: None }
}

/// A page the model looked at: a search result, or a link it read.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Source {
    pub url: String,
    pub title: String,
    /// The site's name as DeepSeek gives it, else the host.
    pub site: String,
    /// The site's icon, when DeepSeek gives one.
    pub icon: String,
}

/// An answer, with what the model looked up on the way (Ask only).
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Reply {
    pub text: String,
    /// Each search it ran, its queries joined with " · ".
    pub searches: Vec<String>,
    /// The pages it found or read, each once, in the order they came.
    pub sources: Vec<Source>,
    /// What to send back with this answer in the next turn (JSON), if any.
    pub carry: Option<String>,
}

/// What the answer is doing while it comes.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The whole answer so far.
    Text(String),
    /// What it's looking up now, in a few words ("Searching: x", "Reading y").
    Looking(String),
}

/// ccez-llm's word budgets: the equivalent's floor, a note, a question.
const GLOSS_WORDS: usize = 8;
const NOTE_WORDS: usize = 20;
const QUESTION_WORDS: usize = 60;
const FOLLOW_UP_WORDS: usize = 90;
/// ccez-lang's budget for explaining a whole line.
const EXPLAIN_WORDS: usize = 170;

/// Words the equivalent may take: the floor, or half again the quote's
/// length; Han and kana count a word per two characters.
fn gloss_words(quote: &str) -> usize {
    let spaced = quote.split_whitespace().count();
    let cjk = quote.chars().filter(|&c| matches!(c as u32, 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)).count();
    let words = spaced.max(cjk.div_ceil(2));
    GLOSS_WORDS.max((words as f64 * 1.5).ceil() as usize)
}

/// A word or phrase in its passage (`context`): a gloss and at most one
/// note, or, with a question, the gloss and the answer.
pub fn ask_messages(context: &str, quote: &str, question: Option<&str>, in_passage_lang: bool) -> Vec<Message> {
    let quote = quote.trim();
    let words = gloss_words(quote);
    let in_passage = "Write in the passage's own language, in plain words an intermediate learner reads easily. Open with a common synonym or the plainest few words for the quote's sense in this passage";
    let english = "Write in English, whatever language the passage is in. Open with the English equivalent of the quote for the sense it has in this passage, the word or short phrase a bilingual dictionary lists for that sense (\"Escalation.\", \"Pounds (sterling).\", \"Unions.\"), or for a clause or sentence a close translation in full: keep the passage's word order and word choices wherever English allows, so the learner can map each piece back to the original; never a polished or reworded English version";
    let equivalent = if in_passage_lang { in_passage } else { english };
    let gloss = format!(
        "The learner is reading the passage right now, so never retell or explain it. {equivalent}: never a definition, never a preamble such as \"Here it means\", and never the quote restated. When the quote is a single inflected verb or participle whose infinitive looks different, add the infinitive in parentheses: \"Crossed (franchir).\" Skip it when it is the quote with only the ending changed (prend, prendre). If a note cites the passage, keep its words as they are. Name a tense, mood or form the way English-language courses for the language name it, with how it is built (\"passé composé: avoir + past participle\", \"Perfekt\", \"te-form\"), never a generic \"past tense\"."
    );
    let user = format!("Paragraph:\n{context}\n\nQuoted: \"{quote}\"");
    match question.map(str::trim).filter(|q| !q.is_empty()) {
        Some(asked) => vec![
            msg(
                "system",
                format!(
                    "You answer a language learner's question about a quoted word or phrase. {gloss} Put it alone on the first line, then answer the question directly on the lines below, in plain connected sentences. No markdown. At most {} words.",
                    words + QUESTION_WORDS
                ),
            ),
            msg("user", format!("{user}\n\nQuestion: {asked}")),
        ],
        None => vec![
            msg(
                "system",
                format!(
                    "You gloss a quoted word or phrase for a language learner who highlighted it because something in it stopped them. {gloss} Then at most one short note, on the single piece of the quote that most likely stopped them, written so they can reuse it: a construction (\"accepter de + infinitive\"), a reference word (\"ce dernier = the latter; feminine cette dernière\"), an idiom, a tense or mood, or a false friend. Give the pattern, not a grammar lesson. When the equivalent already says it all, as for a plain word, stop after it. Never a second note. Put the equivalent alone on the first line and the note on the line below it. No markdown. At most {} words.",
                    words + NOTE_WORDS
                ),
            ),
            msg("user", user),
        ],
    }
}

/// A whole line explained: a natural and a literal translation, the
/// grammar piece by piece, the nuance when there is any. In the line's own
/// language, a plain rewording stands in for the two translations.
pub fn explain_messages(context: &str, line: &str, in_passage_lang: bool) -> Vec<Message> {
    let line = line.trim();
    let (lang, opening) = if in_passage_lang {
        ("the line's own language, in plain words an intermediate learner reads easily", "\"Plain:\" the line said again in simpler words.")
    } else {
        ("English", "\"Natural:\" a natural English translation. \"Literal:\" a word-for-word rendering that shows the structure.")
    };
    vec![
        msg(
            "system",
            format!(
                "You explain one line to a language learner, in {lang}. Use exactly these labelled parts, plain text, no markdown: {opening} \"Grammar:\" two to four lines starting with \"- \", each naming a word or construction from the line (quoted as it is in the line) and what it does; name a tense, mood or form the way English-language courses for the language name it, with how it is built (\"passé composé: avoir + past participle\", \"te-form\"). \"Nuance:\" one or two sentences on tone, register or what is implied, only when there is something worth saying. Use the surrounding lines only to understand the line. At most {EXPLAIN_WORDS} words."
            ),
        ),
        msg("user", format!("{context}\n\nLine: {line}")),
    ]
}

/// A reply in a thread: the first ask (a gloss, or with `explain` the line
/// explained) and its answer replayed, the earlier replies, then the new
/// one. The learner often pushes back; the model weighs it against the
/// passage and says so plainly when they're right.
#[allow(clippy::too_many_arguments)]
pub fn follow_up_messages(
    context: &str,
    quote: &str,
    question: Option<&str>,
    in_passage_lang: bool,
    explain: bool,
    answer: &str,
    thread: &[Turn],
    reply: &str,
) -> Vec<Message> {
    let mut out = if explain { explain_messages(context, quote, in_passage_lang) } else { ask_messages(context, quote, question, in_passage_lang) };
    let follow = format!(
        "The learner may reply to your answer, often to push back. Weigh the reply against the passage itself: walk through how its words build the meaning, piece by piece, closely rather than smoothly, and if the learner is right, say so plainly and correct yourself. Plain connected sentences, no markdown, at most {FOLLOW_UP_WORDS} words per reply."
    );
    out[0].content = format!("{} {follow}", out[0].content);
    out.push(msg("assistant", answer.to_string()));
    for t in thread {
        out.push(msg("user", t.question.clone()));
        out.push(msg("assistant", t.answer.clone()));
    }
    out.push(msg("user", reply.trim().to_string()));
    out
}

/// The `data:` payloads complete so far in an SSE buffer, the rest kept.
/// Bytes, not text: a character split across two network chunks is only
/// decoded once its line is whole.
#[cfg_attr(not(feature = "net"), allow(dead_code))]
pub(crate) fn take_events(buf: &mut Vec<u8>) -> Vec<String> {
    let mut out = Vec::new();
    while let Some(end) = buf.iter().position(|&b| b == b'\n') {
        let raw: Vec<u8> = buf.drain(..=end).collect();
        let line = String::from_utf8_lossy(&raw[..raw.len() - 1]).trim_end_matches('\r').to_string();
        if let Some(data) = line.strip_prefix("data:") {
            out.push(data.trim().to_string());
        }
    }
    out
}

/// DeepSeek's server-side web tools (guides/web_tools): it searches and
/// reads links itself; the client runs nothing. They can't be combined
/// with tools of our own.
fn web_tools() -> serde_json::Value {
    serde_json::json!([{"type": "deepseek_web_search"}, {"type": "deepseek_read_url"}])
}

/// The request body. Thinking is always sent: DeepSeek thinks hard when
/// it's left out. Quick: off. Ask: on at the lowest effort, with the web
/// tools unless `tools` is false (a server that refused them).
pub fn request_body(model: &str, messages: &[Message], mode: Mode, tools: bool) -> serde_json::Value {
    use serde_json::json;
    let ask = mode == Mode::Ask;
    let wire: Vec<serde_json::Value> = messages
        .iter()
        .map(|m| {
            let mut o = json!({"role": m.role, "content": m.content});
            if m.role == "assistant" && ask {
                // An answer's thinking and lookups go back as they came.
                let carried = m.carry.as_deref().and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok());
                if let Some(serde_json::Value::Object(fields)) = carried {
                    for (k, v) in fields {
                        o[k] = v;
                    }
                }
                if o.get("reasoning_content").is_none() {
                    o["reasoning_content"] = json!("");
                }
            }
            o
        })
        .collect();
    let mut body = json!({"model": model, "stream": true, "messages": wire});
    if ask {
        body["thinking"] = json!({"type": "enabled"});
        body["reasoning_effort"] = json!("low");
        if tools {
            body["tools"] = web_tools();
        }
    } else {
        body["thinking"] = json!({"type": "disabled"});
    }
    body
}

/// An error answer in words.
#[cfg_attr(not(feature = "net"), allow(dead_code))]
pub(crate) fn ai_error(status: u16, detail: &str) -> String {
    let said = serde_json::from_str::<serde_json::Value>(detail).ok().and_then(|v| v.pointer("/error/message").and_then(|m| m.as_str()).map(str::to_string));
    match status {
        401 | 403 => "The AI refused the key.".into(),
        402 => "DeepSeek's balance is used up: top it up at platform.deepseek.com.".into(),
        429 => "The AI is busy. Try again in a minute.".into(),
        503 if said.is_none() && detail.contains("no DeepSeek key") => "No DeepSeek key on the Mac yet.".into(),
        _ => format!("The AI answered {status}{}", said.map(|s| format!(": {s}")).unwrap_or_default()),
    }
}

/// The host of `url`, for a source with no site name.
fn host(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let h = rest.split(['/', '?', '#']).next().unwrap_or("");
    h.strip_prefix("www.").unwrap_or(h).to_string()
}

/// One web call's action in a few words, for "what it's looking up".
fn looking(call: &serde_json::Value) -> Option<String> {
    let a = call.get("action")?;
    match a.get("type")?.as_str()? {
        "search" => {
            let qs: Vec<&str> = a.get("queries")?.as_array()?.iter().filter_map(|q| q.as_str()).collect();
            (!qs.is_empty()).then(|| format!("Searching: {}", qs.join(" · ")))
        }
        "open_page" | "read_link" => a.get("url")?.as_str().map(|u| format!("Reading {}", host(u))),
        "find_in_page" => a.get("pattern")?.as_str().map(|p| format!("Finding “{p}”")),
        _ => None,
    }
}

/// The searches and pages in DeepSeek's `server_tool_calls`, added to
/// `reply` (each page once).
fn take_lookups(steps: &serde_json::Value, reply: &mut Reply) {
    let add = |reply: &mut Reply, s: &serde_json::Value| {
        let Some(url) = s.get("url").and_then(|u| u.as_str()).filter(|u| !u.is_empty()) else { return };
        if reply.sources.iter().any(|x| x.url == url) {
            return;
        }
        let text = |k: &str| s.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let site = Some(text("site_name")).filter(|t| !t.is_empty()).unwrap_or_else(|| host(url));
        let title = Some(text("title")).filter(|t| !t.is_empty()).unwrap_or_else(|| url.to_string());
        reply.sources.push(Source { url: url.to_string(), title, site, icon: text("site_icon") });
    };
    for step in steps.as_array().into_iter().flatten() {
        for call in step.get("calls").and_then(|c| c.as_array()).into_iter().flatten() {
            if let Some(a) = call.get("action").filter(|a| a.get("type").and_then(|t| t.as_str()) == Some("search")) {
                let qs: Vec<&str> = a.get("queries").and_then(|q| q.as_array()).into_iter().flatten().filter_map(|q| q.as_str()).collect();
                if !qs.is_empty() {
                    reply.searches.push(qs.join(" · "));
                }
            }
            for s in call.get("sources").and_then(|s| s.as_array()).into_iter().flatten() {
                add(reply, s);
            }
            if let Some(page) = call.get("page") {
                add(reply, page);
            }
        }
    }
}

/// Reads one streamed chunk into `reply`; Err when the server reports a
/// failure mid-answer (DeepSeek: `error` with no choices, no [DONE]).
#[cfg_attr(not(feature = "net"), allow(dead_code))]
pub(crate) fn take_chunk(v: &serde_json::Value, reply: &mut Reply, thinking: &mut String, steps: &mut Option<serde_json::Value>, on: &mut dyn FnMut(Event)) -> Result<(), String> {
    if let Some(e) = v.get("error").filter(|e| !e.is_null()) {
        return Err(e.get("message").and_then(|m| m.as_str()).unwrap_or("the answer broke off").to_string());
    }
    let Some(choice) = v.pointer("/choices/0") else { return Ok(()) };
    if let Some(call) = choice.get("server_tool_event").filter(|e| e.get("type").and_then(|t| t.as_str()) == Some("call_started")).and_then(|e| e.get("call"))
        && let Some(line) = looking(call)
    {
        on(Event::Looking(line));
    }
    let Some(delta) = choice.get("delta") else { return Ok(()) };
    if let Some(r) = delta.get("reasoning_content").and_then(|r| r.as_str()) {
        thinking.push_str(r);
    }
    if let Some(piece) = delta.get("content").and_then(|p| p.as_str())
        && !piece.is_empty()
    {
        reply.text.push_str(piece);
        on(Event::Text(reply.text.clone()));
    }
    if let Some(calls) = delta.get("server_tool_calls").filter(|c| c.is_array()) {
        take_lookups(calls, reply);
        *steps = Some(calls.clone());
    }
    if let Some(notes) = delta.get("annotations").and_then(|a| a.as_array()) {
        // Pages cited but not among the lookups (never expected; kept so
        // nothing cited goes unshown).
        for n in notes {
            if let Some(c) = n.get("url_citation") {
                take_lookups(&serde_json::json!([{"calls": [{"page": c}]}]), reply);
            }
        }
    }
    Ok(())
}

/// Asks the AI, `on` hearing the answer as it grows and what it's
/// looking up. Ask mode falls back to no web tools once if the server
/// refuses them (400/422). Needs a Tokio runtime.
#[cfg(feature = "net")]
pub async fn chat(ai: &Ai, messages: &[Message], mode: Mode, mut on: impl FnMut(Event) + Send) -> crate::Result<Reply> {
    match chat_once(ai, messages, mode, true, &mut on).await {
        Err(Failed(_, true)) if mode == Mode::Ask => chat_once(ai, messages, mode, false, &mut on).await.map_err(|e| e.0),
        r => r.map_err(|e| e.0),
    }
}

/// An error, and whether the server refused the request's shape.
#[cfg(feature = "net")]
struct Failed(crate::LangError, bool);

#[cfg(feature = "net")]
async fn chat_once(ai: &Ai, messages: &[Message], mode: Mode, tools: bool, on: &mut (dyn FnMut(Event) + Send)) -> Result<Reply, Failed> {
    use crate::LangError;
    let body = request_body(&ai.model, messages, mode, tools);
    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        // Web lookups take a while before the first word.
        .timeout(std::time::Duration::from_secs(if mode == Mode::Ask { 240 } else { 90 }))
        .build()
        .unwrap_or_default();
    let mut req = http
        .post(format!("{}/chat/completions", ai.url.trim_end_matches('/')))
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
        .body(body.to_string());
    if !ai.key.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", ai.key));
    }
    let net = |e: reqwest::Error| Failed(if e.is_timeout() { LangError::Timeout } else { LangError::Network }, false);
    let mut res = req.send().await.map_err(net)?;
    if !res.status().is_success() {
        let status = res.status().as_u16();
        let detail = res.text().await.unwrap_or_default();
        return Err(Failed(LangError::Ai { reason: ai_error(status, &detail) }, tools && matches!(status, 400 | 422)));
    }
    let mut buf: Vec<u8> = Vec::new();
    let mut reply = Reply::default();
    let mut thinking = String::new();
    let mut steps = None;
    let finish = |reply: &mut Reply, thinking: String, steps: Option<serde_json::Value>| {
        if mode == Mode::Ask && (steps.is_some() || !thinking.is_empty()) {
            let mut carry = serde_json::json!({"reasoning_content": thinking});
            if let Some(s) = steps {
                carry["server_tool_calls"] = s;
            }
            reply.carry = Some(carry.to_string());
        }
    };
    while let Some(chunk) = res.chunk().await.map_err(net)? {
        buf.extend_from_slice(&chunk);
        for data in take_events(&mut buf) {
            if data == "[DONE]" {
                finish(&mut reply, thinking, steps);
                return Ok(reply);
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) else { continue };
            take_chunk(&v, &mut reply, &mut thinking, &mut steps, on).map_err(|reason| Failed(LangError::Ai { reason }, false))?;
        }
    }
    finish(&mut reply, thinking, steps);
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(role: &str, content: &str) -> Message {
        msg(role, content.into())
    }

    #[test]
    fn deepseek_flash_is_the_default_and_blank_settings_take_it() {
        let ai = Ai::new(" ", "", " sk-test-fake ");
        assert_eq!(ai, Ai { url: "https://api.deepseek.com".into(), model: "deepseek-flash".into(), key: "sk-test-fake".into() });
        assert_eq!(Ai::new("https://z.example:8448/ai/deepseek/", "other-model", "").url, "https://z.example:8448/ai/deepseek");
    }

    #[test]
    fn quick_work_turns_thinking_off_and_offers_no_tools() {
        let b = request_body("deepseek-flash", &[m("system", "s"), m("user", "u")], Mode::Quick, true);
        assert_eq!(b["thinking"], serde_json::json!({"type": "disabled"}));
        assert!(b.get("reasoning_effort").is_none() && b.get("tools").is_none());
        assert_eq!(b["model"], "deepseek-flash");
        assert_eq!(b["stream"], true);
        assert_eq!(b["messages"][1], serde_json::json!({"role": "user", "content": "u"}));
    }

    #[test]
    fn ask_thinks_lightly_with_the_web_tools_and_sends_answers_back_whole() {
        let mut earlier = m("assistant", "A1");
        earlier.carry = Some(r#"{"reasoning_content":"r","server_tool_calls":[{"index":0,"calls":[]}]}"#.into());
        let b = request_body("deepseek-flash", &[m("user", "Q1"), earlier, m("assistant", "A2"), m("user", "Q3")], Mode::Ask, true);
        assert_eq!(b["thinking"], serde_json::json!({"type": "enabled"}));
        assert_eq!(b["reasoning_effort"], "low");
        assert_eq!(b["tools"], serde_json::json!([{"type": "deepseek_web_search"}, {"type": "deepseek_read_url"}]));
        assert_eq!(b["messages"][1]["reasoning_content"], "r");
        assert_eq!(b["messages"][1]["server_tool_calls"][0]["index"], 0);
        assert_eq!(b["messages"][2]["reasoning_content"], "");
        assert!(b["messages"][0].get("reasoning_content").is_none());
        assert!(request_body("deepseek-flash", &[], Mode::Ask, false).get("tools").is_none());
    }

    #[test]
    fn errors_name_the_key_the_balance_and_the_wait() {
        assert_eq!(ai_error(401, ""), "The AI refused the key.");
        assert!(ai_error(402, "").contains("balance is used up"));
        assert!(ai_error(429, "").contains("busy"));
        assert_eq!(ai_error(400, r#"{"error":{"message":"bad"}}"#), "The AI answered 400: bad");
    }

    #[test]
    fn a_stream_brings_text_lookups_and_what_to_send_back() {
        let chunks = [
            r#"{"choices":[{"index":0,"delta":{"reasoning_content":"hm"}}]}"#,
            r#"{"choices":[{"index":0,"delta":{},"server_tool_event":{"type":"call_started","step":0,"call":{"index":0,"type":"deepseek_web_search","kind":"deep_search","action":{"type":"search","queries":["argot 2026","verlan"]}}}}]}"#,
            r#"{"choices":[{"index":0,"delta":{},"server_tool_event":{"type":"call_started","step":0,"call":{"action":{"type":"read_link","url":"https://www.example.fr/a"}}}}]}"#,
            r#"{"choices":[{"index":0,"delta":{"content":"Ça "}}]}"#,
            r#"{"choices":[{"index":0,"delta":{"content":"va.","annotations":[{"type":"url_citation","url_citation":{"url":"https://cited.example/x","title":"Cited","start_index":0,"end_index":3}}],"server_tool_calls":[{"index":0,"reasoning_content":"hm","calls":[{"index":0,"type":"deepseek_web_search","kind":"deep_search","action":{"type":"search","queries":["argot 2026","verlan"]},"status":"completed","sources":[{"type":"url","url":"https://www.example.fr/a","title":"A","site_name":"Exemple","site_icon":"https://cdn.example/i.png"},{"type":"url","url":"https://b.example/b","title":"B"}]},{"index":1,"type":"deepseek_read_url","kind":"read_link","action":{"type":"read_link","url":"https://www.example.fr/a"},"status":"completed","page":{"url":"https://www.example.fr/a","title":"A"},"signature":"sig"}]}]},"finish_reason":"stop"}]}"#,
        ];
        let (mut reply, mut thinking, mut steps, mut seen) = (Reply::default(), String::new(), None, Vec::new());
        for c in chunks {
            let v: serde_json::Value = serde_json::from_str(c).unwrap();
            take_chunk(&v, &mut reply, &mut thinking, &mut steps, &mut |e| seen.push(e)).unwrap();
        }
        assert_eq!(reply.text, "Ça va.");
        assert_eq!(seen[0], Event::Looking("Searching: argot 2026 · verlan".into()));
        assert_eq!(seen[1], Event::Looking("Reading example.fr".into()));
        assert_eq!(seen.last(), Some(&Event::Text("Ça va.".into())));
        assert_eq!(reply.searches, ["argot 2026 · verlan"]);
        let urls: Vec<&str> = reply.sources.iter().map(|s| s.url.as_str()).collect();
        assert_eq!(urls, ["https://www.example.fr/a", "https://b.example/b", "https://cited.example/x"]);
        assert_eq!((reply.sources[0].site.as_str(), reply.sources[0].icon.as_str()), ("Exemple", "https://cdn.example/i.png"));
        assert_eq!((reply.sources[1].site.as_str(), reply.sources[1].title.as_str()), ("b.example", "B"));
        assert_eq!(reply.sources[2].title, "Cited");
        assert_eq!(thinking, "hm");
        assert_eq!(steps.unwrap()[0]["calls"][1]["signature"], "sig");
    }

    #[test]
    fn a_failure_mid_answer_is_an_error_not_a_cut() {
        let v: serde_json::Value = serde_json::from_str(r#"{"choices":[],"error":{"message":"The model failed to generate a complete response."}}"#).unwrap();
        let r = take_chunk(&v, &mut Reply::default(), &mut String::new(), &mut None, &mut |_| {});
        assert_eq!(r, Err("The model failed to generate a complete response.".into()));
    }

    #[test]
    fn a_tap_without_a_question_asks_for_a_gloss() {
        let m = ask_messages("Je vais à la plage.\nIl fait beau.", "plage", None, false);
        assert_eq!(m.len(), 2);
        assert!(m[0].content.starts_with("You gloss a quoted word"));
        assert!(m[0].content.ends_with("At most 28 words."));
        assert_eq!(m[1].content, "Paragraph:\nJe vais à la plage.\nIl fait beau.\n\nQuoted: \"plage\"");
    }

    #[test]
    fn follow_ups_replay_the_thread() {
        let m = follow_up_messages("p", "q", None, false, false, "A1", &[Turn { question: "Q2".into(), answer: "A2".into() }], "Q3");
        let roles: Vec<&str> = m.iter().map(|x| x.role.as_str()).collect();
        assert_eq!(roles, ["system", "user", "assistant", "user", "assistant", "user"]);
        assert!(m[0].content.contains("push back"));
    }

    #[test]
    fn a_line_is_explained_in_labelled_parts() {
        let m = explain_messages("Il fait beau.", " Je vais à la plage. ", false);
        assert!(m[0].content.contains("\"Natural:\"") && m[0].content.contains("\"Grammar:\""));
        assert!(m[1].content.ends_with("\n\nLine: Je vais à la plage."));
    }

    #[test]
    fn sse_events_come_out_whole() {
        let mut buf = b"data: {\"a\":1}\n\ndata: [DO".to_vec();
        assert_eq!(take_events(&mut buf), ["{\"a\":1}"]);
        buf.extend_from_slice(b"NE]\n");
        assert_eq!(take_events(&mut buf), ["[DONE]"]);
        let line = "data: 音楽\n".as_bytes();
        let mut buf = line[..8].to_vec();
        assert!(take_events(&mut buf).is_empty());
        buf.extend_from_slice(&line[8..]);
        assert_eq!(take_events(&mut buf), ["音楽"]);
    }

    #[test]
    fn japanese_counts_two_characters_a_word() {
        assert_eq!(gloss_words("音楽"), 8);
    }
}
