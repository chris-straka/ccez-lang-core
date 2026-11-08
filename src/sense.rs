//! Which of a word's dictionary meanings a line most likely means, at
//! once, from the line's translation (owner, 2026-11-03: ccez-llm shows
//! a meaning the moment a word is tapped; the AI's comes later): the
//! meaning sharing the most words with it. 勝つ in "when I won at club"
//! picks nothing by word ("to win"), but 寒い in "because it's cold"
//! picks "cold" over "chilly".

/// Words in a meaning that say nothing about which sense it is.
const FILLER: &[&str] = &[
    "a", "an", "the", "to", "of", "in", "on", "at", "for", "with", "by", "from", "into", "up", "out", "as", "or", "and",
    "be", "is", "are", "was", "it", "its", "this", "that", "one", "one's", "someone", "something", "oneself", "etc",
    "very", "not", "no", "do", "have", "get", "make",
];

/// A word cut to what its forms share: "winning", "wins" → "win".
fn stem(word: &str) -> String {
    let w = word.to_lowercase();
    for end in ["ing", "ed", "es", "s", "ly"] {
        if let Some(s) = w.strip_suffix(end)
            && s.chars().count() >= 3
        {
            // "winning" → "winn" → "win".
            let c: Vec<char> = s.chars().collect();
            // By characters, not bytes: "ññ" is two bytes each.
            let s = if c.len() >= 4 && c[c.len() - 1] == c[c.len() - 2] && !"aeiou".contains(c[c.len() - 1]) { &s[..s.len() - c[c.len() - 1].len_utf8()] } else { s };
            return s.trim_end_matches('e').to_string();
        }
    }
    w.trim_end_matches('e').to_string()
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|w| !w.is_empty() && !FILLER.contains(&w.to_lowercase().as_str()))
        .map(stem)
        .collect()
}

/// The index of the meaning in `meanings` most like `translation`, or
/// None when none shares a word with it. Ties go to the earlier meaning
/// (dictionaries list the common sense first).
pub fn likely(meanings: &[String], translation: &str) -> Option<usize> {
    let said = words(translation);
    if said.is_empty() {
        return None;
    }
    meanings
        .iter()
        .enumerate()
        .map(|(i, m)| {
            // What's in brackets is a usage note, not the sense.
            let plain: String = m.split('(').next().unwrap_or(m).to_string();
            (i, words(&plain).iter().filter(|w| said.contains(w)).count())
        })
        .filter(|&(_, n)| n > 0)
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_meaning_the_translation_uses_wins() {
        let banque = m(&["bank (financial institution)", "bench", "bank (of a river)"]);
        assert_eq!(likely(&banque, "I sat on the bench"), Some(1));
        let samui = m(&["chilly", "cold (weather)", "poor; lacking"]);
        assert_eq!(likely(&samui, "Because it's cold"), Some(1));
        // A form of the word: "winning" against "to win".
        assert_eq!(likely(&m(&["to lose", "to win"]), "They keep winning"), Some(1));
    }

    #[test]
    fn nothing_shared_is_no_guess() {
        assert_eq!(likely(&m(&["to win; to gain victory"]), "When I won at club"), None);
        assert_eq!(likely(&m(&["to win"]), ""), None);
        // Filler alone matches nothing.
        assert_eq!(likely(&m(&["to be"]), "to be or not"), None);
    }
}
