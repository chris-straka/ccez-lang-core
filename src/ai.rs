//! The learner's AI (Muse by default, their own key): the providers,
//! the prompts the apps share, and an OpenAI-compatible `chat/completions`
//! client that streams the answer. The prompts are ccez-llm's annotation
//! prompts (`src/lib/reading.ts`) as ccez-tube ported them, plus the
//! line explanation from ccez-lang (`src/core/ai/prompts.ts`).

/// One AI service. `thinking` is the provider's own levels, as ccez-llm
/// sends them; the first `default_thinking` is used when the app has no
/// choice saved.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Provider {
    pub id: &'static str,
    pub label: &'static str,
    pub url: &'static str,
    pub model: &'static str,
    pub key_hint: &'static str,
    pub thinking: &'static [&'static str],
    pub default_thinking: Option<&'static str>,
}

/// Muse first: the default.
pub const PROVIDERS: &[Provider] = &[
    Provider {
        id: "muse",
        label: "Muse",
        url: "https://api.meta.ai/v1",
        model: "muse-spark-1.3-contributor",
        key_hint: "Meta Model API key",
        thinking: &["minimal", "low", "medium", "high", "xhigh"],
        default_thinking: Some("medium"),
    },
    Provider {
        id: "deepseek",
        label: "DeepSeek",
        url: "https://api.deepseek.com",
        model: "deepseek-flash",
        key_hint: "Starts with sk-",
        thinking: &["off", "high", "max"],
        default_thinking: Some("high"),
    },
    Provider {
        id: "zen",
        label: "OpenCode Zen",
        url: "https://opencode.ai/zen/v1",
        model: "big-pickle",
        key_hint: "Zen key from opencode.ai/auth",
        thinking: &[],
        default_thinking: None,
    },
    Provider { id: "custom", label: "Your own server", url: "", model: "", key_hint: "If it needs one", thinking: &[], default_thinking: None },
];

pub fn provider(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|p| p.id == id)
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Message {
    pub role: String,
    pub content: String,
}

/// One earlier exchange in a thread: what the learner wrote, the answer.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Turn {
    pub question: String,
    pub answer: String,
}

pub(crate) fn msg(role: &str, content: String) -> Message {
    Message { role: role.into(), content }
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

/// The request body: the messages, streamed, with the provider's own
/// thinking switch (Muse's `reasoning_effort`, which has no "off";
/// DeepSeek's thinking on or off).
pub fn request_body(base_url: &str, model: &str, messages: &[Message], thinking: Option<&str>) -> serde_json::Value {
    use serde_json::json;
    let mut body = json!({
        "model": model,
        "stream": true,
        "messages": messages.iter().map(|m| json!({"role": m.role, "content": m.content})).collect::<Vec<_>>(),
    });
    match (thinking, base_url.contains("api.meta.ai"), base_url.contains("deepseek")) {
        (Some(level), true, _) => body["reasoning_effort"] = json!(level),
        (Some("off"), _, true) => body["thinking"] = json!({"type": "disabled"}),
        (Some(level @ ("high" | "max")), _, true) => {
            body["thinking"] = json!({"type": "enabled"});
            body["reasoning_effort"] = json!(level);
        }
        _ => {}
    }
    body
}

/// An error answer in words.
#[cfg_attr(not(feature = "net"), allow(dead_code))]
fn ai_error(status: u16, detail: &str) -> String {
    let said = serde_json::from_str::<serde_json::Value>(detail).ok().and_then(|v| v.pointer("/error/message").and_then(|m| m.as_str()).map(str::to_string));
    match status {
        401 | 403 => "The AI refused the key. Check it in settings.".into(),
        429 => "The AI is busy or the key's limit is reached. Try again in a minute.".into(),
        _ => format!("The AI answered {status}{}", said.map(|s| format!(": {s}")).unwrap_or_default()),
    }
}

/// Asks `model` at `base_url` (OpenAI-compatible) with the learner's
/// `key`, calling `on_text` with the whole answer so far as each piece
/// arrives. Returns the whole answer. Needs a Tokio runtime.
#[cfg(feature = "net")]
pub async fn ask(
    base_url: &str,
    key: &str,
    model: &str,
    messages: &[Message],
    thinking: Option<&str>,
    mut on_text: impl FnMut(&str) + Send,
) -> crate::Result<String> {
    use crate::LangError;
    let body = request_body(base_url, model, messages, thinking);
    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .unwrap_or_default();
    let mut req = http
        .post(format!("{}/chat/completions", base_url.trim_end_matches('/')))
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
        .body(body.to_string());
    if !key.is_empty() {
        req = req.header("Authorization", format!("Bearer {key}"));
    }
    let net = |e: reqwest::Error| if e.is_timeout() { LangError::Timeout } else { LangError::Network };
    let mut res = req.send().await.map_err(net)?;
    if !res.status().is_success() {
        let status = res.status().as_u16();
        let detail = res.text().await.unwrap_or_default();
        return Err(LangError::Ai { reason: ai_error(status, &detail) });
    }
    let mut buf: Vec<u8> = Vec::new();
    let mut answer = String::new();
    while let Some(chunk) = res.chunk().await.map_err(net)? {
        buf.extend_from_slice(&chunk);
        for data in take_events(&mut buf) {
            if data == "[DONE]" {
                return Ok(answer);
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) else { continue };
            if let Some(piece) = v.pointer("/choices/0/delta/content").and_then(|p| p.as_str())
                && !piece.is_empty()
            {
                answer.push_str(piece);
                on_text(&answer);
            }
        }
    }
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn muse_is_the_default_and_its_thinking_has_no_off() {
        assert_eq!(PROVIDERS[0].id, "muse");
        assert_eq!(PROVIDERS[0].model, "muse-spark-1.3-contributor");
        assert!(!PROVIDERS[0].thinking.contains(&"off"));
        let b = request_body(PROVIDERS[0].url, PROVIDERS[0].model, &[], Some("minimal"));
        assert_eq!(b["reasoning_effort"], "minimal");
        let d = request_body("https://api.deepseek.com", "deepseek-flash", &[], Some("off"));
        assert_eq!(d["thinking"]["type"], "disabled");
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
