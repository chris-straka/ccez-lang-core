//! Corrections of what a learner wrote, the way ccez-llm's correction
//! mode shows them (`src/lib/correction.ts`): the model puts the
//! corrected full text in a ```correction fenced block, and the learner
//! sees their text as a word diff, what goes crossed out, what comes in
//! marked. Han, kana and Hangul diff character by character.

use crate::ai::{Message, msg};

/// ccez-llm's line for a chat with correction on, word for word.
pub const CORRECTION_HINT: &str = "Also correct the user's last message: append a ```correction fenced block holding the corrected full text and nothing else (repeat it unchanged when it needs no correction; ignore any 'Annotated selections:' block when correcting).";

/// The corrected text out of a model's answer, and the answer without
/// it. Closed fences leave the body; an unclosed one (mid-stream) hides
/// the rest, and its partial text still counts. Several blocks join.
pub fn extract_correction(markdown: &str) -> (Option<String>, String) {
    let lower = markdown.to_lowercase();
    let mut corrections: Vec<String> = Vec::new();
    let mut body = String::new();
    let mut at = 0;
    let mut search = 0;
    while let Some(rel) = lower[search..].find("```") {
        let start = search + rel;
        let after = &lower[start + 3..];
        let info = after.trim_start_matches([' ', '\t']);
        if !info.starts_with("correction") {
            search = start + 3;
            continue;
        }
        let mut inner = start + 3 + (after.len() - info.len()) + "correction".len();
        inner += lower[inner..].len() - lower[inner..].trim_start_matches([' ', '\t']).len();
        if lower[inner..].starts_with("\r\n") {
            inner += 2;
        } else if lower[inner..].starts_with('\n') {
            inner += 1;
        }
        let close = markdown[inner..].find("```").map(|c| inner + c);
        let end = close.unwrap_or(markdown.len());
        let text = markdown[inner..end].trim();
        if !text.is_empty() {
            corrections.push(text.to_string());
        }
        body.push_str(&markdown[at..start]);
        at = close.map_or(markdown.len(), |c| c + 3);
        search = at;
        if close.is_none() {
            break;
        }
    }
    if corrections.is_empty() && at == 0 {
        return (None, markdown.to_string());
    }
    body.push_str(&markdown[at.min(markdown.len())..]);
    ((!corrections.is_empty()).then(|| corrections.join("\n\n")), body.trim().to_string())
}

/// One run of a diff: unchanged, the learner's (to drop), or the fix's.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum Op {
    Same { text: String },
    Del { text: String },
    Ins { text: String },
}

fn cjk(c: char) -> bool {
    matches!(c as u32, 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xAC00..=0xD7AF | 0x1100..=0x11FF | 0x3130..=0x318F | 0x20000..=0x2FA1F)
}

/// Word tokens for spaced scripts, each carrying its trailing spaces (so
/// a swapped word rejoins readable), single characters for Han, kana and
/// Hangul. The tokens rejoin to the exact input.
pub fn tokens(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut spaces = String::new();
    for ch in text.chars() {
        if cjk(ch) {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
            if !spaces.is_empty() {
                out.push(std::mem::take(&mut spaces));
            }
            out.push(ch.to_string());
            continue;
        }
        if ch.is_whitespace() {
            if word.is_empty() {
                spaces.push(ch);
            } else {
                word.push(ch);
            }
            continue;
        }
        if word.ends_with(char::is_whitespace) {
            out.push(std::mem::take(&mut word));
        }
        if !spaces.is_empty() {
            out.push(std::mem::take(&mut spaces));
        }
        word.push(ch);
    }
    if !word.is_empty() {
        out.push(word);
    }
    if !spaces.is_empty() {
        out.push(spaces);
    }
    out
}

/// The smallest token diff from `before` to `after`; runs of the same
/// kind merge.
pub fn diff(before: &str, after: &str) -> Vec<Op> {
    let a = tokens(before);
    let b = tokens(after);
    let (n, m) = (a.len(), b.len());
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    let mut ops: Vec<Op> = Vec::new();
    let mut push = |op: Op| {
        match (ops.last_mut(), &op) {
            (Some(Op::Same { text }), Op::Same { text: t }) | (Some(Op::Del { text }), Op::Del { text: t }) | (Some(Op::Ins { text }), Op::Ins { text: t }) => {
                text.push_str(t)
            }
            _ => ops.push(op),
        }
    };
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            push(Op::Same { text: a[i].clone() });
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            push(Op::Del { text: a[i].clone() });
            i += 1;
        } else {
            push(Op::Ins { text: b[j].clone() });
            j += 1;
        }
    }
    for t in &a[i..] {
        push(Op::Del { text: t.clone() });
    }
    for t in &b[j..] {
        push(Op::Ins { text: t.clone() });
    }
    ops
}

/// A correction to show: the fixed text, why (one short line, when the
/// model gave one), and the diff from what was written.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Correction {
    pub text: String,
    pub why: Option<String>,
    pub ops: Vec<Op>,
}

/// English names for the prompt.
pub fn language_name(code: &str) -> &str {
    match lexicon::base_lang(code).as_str() {
        "fr" => "French",
        "de" => "German",
        "es" => "Spanish",
        "ja" => "Japanese",
        "zh" => "Chinese",
        "ko" => "Korean",
        "pt" => "Portuguese",
        "ar" => "Arabic",
        "hi" => "Hindi",
        "ru" => "Russian",
        "pl" => "Polish",
        "id" => "Indonesian",
        "tr" => "Turkish",
        "it" => "Italian",
        "no" => "Norwegian",
        "cs" => "Czech",
        "el" => "Greek",
        "fa" => "Persian",
        "th" => "Thai",
        "vi" => "Vietnamese",
        "ro" => "Romanian",
        "bg" => "Bulgarian",
        "hu" => "Hungarian",
        "hy" => "Armenian",
        "nl" => "Dutch",
        "sv" => "Swedish",
        "uk" => "Ukrainian",
        "he" => "Hebrew",
        "en" => "English",
        _ => "the language it is written in",
    }
}

/// What a learner just wrote in `lang` (a message being typed), to be
/// corrected the way ccez-llm does, plus why in `explain_in` (the
/// learner's own language): a correction block, then one short line.
/// `context` is what they're replying to, when known.
pub fn correction_messages(lang: &str, text: &str, explain_in: &str, context: Option<&str>) -> Vec<Message> {
    let language = language_name(lang);
    let explain = language_name(explain_in);
    let system = format!(
        "You correct a language learner's writing in {language} as they type a message. Reply with a ```correction fenced block holding the corrected full text and nothing else (repeat it unchanged when it needs no correction). Fix only mistakes: grammar, spelling, accents, agreement, a word that doesn't say what they mean, or a phrasing no native speaker would use; keep their words, tone and register otherwise, and leave slang and casual spelling that natives use. When you changed something, add one line after the block, in {explain}, at most 14 words, naming the rule the way courses for the language name it (\"avoir hâte: hâte is a noun\", \"te-form + いる for an ongoing action\"). Nothing else, no markdown besides the block."
    );
    let user = match context.map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) => format!("Replying to: {c}\n\nWritten: {}", text.trim()),
        None => format!("Written: {}", text.trim()),
    };
    vec![msg("system", system), msg("user", user)]
}

/// The model's answer to [`correction_messages`] as a correction, or
/// None when it changed nothing (or gave no block).
pub fn parse_correction(written: &str, answer: &str) -> Option<Correction> {
    let (fixed, rest) = extract_correction(answer);
    let fixed = fixed?.trim().to_string();
    let written = written.trim();
    if fixed == written {
        return None;
    }
    let ops = diff(written, &fixed);
    if ops.iter().all(|o| matches!(o, Op::Same { .. })) {
        return None;
    }
    let why = rest.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string);
    Some(Correction { text: fixed, why, ops })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> Op {
        Op::Same { text: t.into() }
    }
    fn d(t: &str) -> Op {
        Op::Del { text: t.into() }
    }
    fn i(t: &str) -> Op {
        Op::Ins { text: t.into() }
    }

    #[test]
    fn tokens_rejoin_to_the_input() {
        for text in ["je suis hâte de te voir", "  deux  espaces ", "明日、ひまですか？", "밥을 먹어요"] {
            assert_eq!(tokens(text).concat(), text);
        }
        assert_eq!(tokens("je suis"), ["je ", "suis"]);
        assert_eq!(tokens("猫が好き"), ["猫", "が", "好", "き"]);
    }

    #[test]
    fn french_diffs_word_by_word() {
        assert_eq!(diff("je suis hâte de te voir", "j'ai hâte de te voir"), [d("je suis "), i("j'ai "), s("hâte de te voir")]);
    }

    #[test]
    fn japanese_and_chinese_diff_by_character() {
        assert_eq!(diff("猫が好きです", "猫は好きです"), [s("猫"), d("が"), i("は"), s("好きです")]);
        assert_eq!(diff("我很喜欢吃面", "我很喜欢吃面条"), [s("我很喜欢吃面"), i("条")]);
    }

    #[test]
    fn the_block_comes_out_of_the_answer() {
        let (fix, rest) = extract_correction("```correction\nj'ai hâte de te voir\n```\navoir hâte: hâte is a noun");
        assert_eq!(fix.as_deref(), Some("j'ai hâte de te voir"));
        assert_eq!(rest, "avoir hâte: hâte is a noun");
        // Mid-stream, the open block still gives its text so far.
        let (fix, rest) = extract_correction("Bien !\n```Correction\nj'ai hâ");
        assert_eq!(fix.as_deref(), Some("j'ai hâ"));
        assert_eq!(rest, "Bien !");
        assert_eq!(extract_correction("pas de bloc").0, None);
    }

    #[test]
    fn an_unchanged_text_is_no_correction() {
        assert_eq!(parse_correction("Bonjour à tous", "```correction\nBonjour à tous\n```"), None);
        let c = parse_correction("je suis hâte de te voir", "```correction\nj'ai hâte de te voir\n```\navoir hâte: hâte is a noun\n").unwrap();
        assert_eq!(c.text, "j'ai hâte de te voir");
        assert_eq!(c.why.as_deref(), Some("avoir hâte: hâte is a noun"));
        assert_eq!(c.ops[1], i("j'ai "));
    }

    #[test]
    fn the_request_names_the_language_and_the_context() {
        let m = correction_messages("fr-CA", "je suis hâte", "en", Some("On se voit quand ?"));
        assert!(m[0].content.contains("writing in French"));
        assert!(m[0].content.contains("in English, at most 14 words"));
        assert_eq!(m[1].content, "Replying to: On se voit quand ?\n\nWritten: je suis hâte");
    }
}
