//! Dictionaries packed and read the way an app does, from small files in
//! ccez-lang's formats (tests/fixtures; see CREDITS.md): French
//! (Wiktionary via kaikki), Chinese (CC-CEDICT), Japanese (JMdict).

use std::path::{Path, PathBuf};

use ccez_lang_core::Words;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

fn words() -> Words {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("ccez-lang-words-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let w = Words::new(&root);
    w.install("fr", &fixture("fr.tsv")).unwrap();
    w.install("de", &fixture("de.tsv")).unwrap();
    w.install("zh", &fixture("cedict.txt")).unwrap();
    w.install("ja", &fixture("ja.tsv")).unwrap();
    w
}

#[test]
fn french_entries_bring_gender_article_and_lemma() {
    let w = words();
    assert!(w.ready("fr-CA"));
    let maison = w.lookup("fr", "maison", None).unwrap();
    assert_eq!(maison.senses[0].meanings, ["house"]);
    assert_eq!(maison.gender.as_deref(), Some("feminine"));
    assert_eq!(maison.article.as_deref(), Some("la"));
    assert_eq!(maison.ipa.as_deref(), Some("/mɛ.zɔ̃/"));
    // A form points at its dictionary word.
    let allons = w.lookup("fr", "allons", None).unwrap();
    assert_eq!(allons.lemma.as_deref(), Some("aller"));
    assert!(allons.senses[0].meanings.iter().any(|m| m == "to go"));
    // Elision and a capital at the start of a sentence.
    assert_eq!(w.lookup("fr", "L'hôtel", None).unwrap().word, "hôtel");
    assert_eq!(w.lookup("fr", "Maison", None).unwrap().senses[0].meanings, ["house"]);
    assert!(w.lookup("fr", "zzzz", None).is_none());
}

#[test]
fn french_lines_keep_every_character() {
    let w = words();
    let line = "« J'ai hâte de te voir », dit-elle.";
    let t = w.tokens("fr", line);
    assert_eq!(t.iter().map(|t| t.text.as_str()).collect::<String>(), line);
    let words: Vec<&str> = t.iter().filter(|t| t.word).map(|t| t.text.as_str()).collect();
    assert_eq!(words, ["J'ai", "hâte", "de", "te", "voir", "dit", "elle"]);
}

#[test]
fn chinese_is_cut_into_words_with_pinyin_over_each_character() {
    let w = words();
    let t = w.tokens("zh-Hans", "我喜欢猫。");
    let cut: Vec<(&str, bool)> = t.iter().map(|t| (t.text.as_str(), t.word)).collect();
    assert_eq!(cut, [("我", true), ("喜欢", true), ("猫", true), ("。", false)]);
    let xihuan = &t[1];
    assert_eq!(xihuan.reading.as_deref(), Some("xǐ huan"));
    assert_eq!(xihuan.ruby.iter().map(|p| p.rt.as_deref().unwrap_or("")).collect::<Vec<_>>(), ["xǐ", "huan"]);
    let mao = w.lookup("zh", "猫", None).unwrap();
    assert_eq!(mao.word, "猫 (貓)");
    assert_eq!(mao.reading.as_deref(), Some("māo"));
    assert_eq!(w.sound("zh", "我喜欢猫"), "wǒxǐhuanmāo");
}

#[test]
fn japanese_entries_from_jmdict() {
    let w = words();
    let neko = w.lookup("ja", "猫", None).unwrap();
    assert_eq!(neko.reading.as_deref(), Some("ねこ"));
    assert!(neko.common);
    assert_eq!(neko.senses[0].meanings[0], "cat");
    // Katakana finds the hiragana headword.
    assert_eq!(w.lookup("ja", "ネコ", None).unwrap().senses[0].meanings[0], "cat");
    // Without the analyser (no `ja` feature, or no IPAdic files), a
    // Japanese line is still cut and kept whole.
    let line = "猫が好きです。";
    assert_eq!(w.tokens("ja", line).iter().map(|t| t.text.as_str()).collect::<String>(), line);
}

#[test]
fn furigana_and_pinyin_helpers() {
    use ccez_lang_core::words::{furigana, numbered_to_marks, to_hiragana};
    let parts = furigana("食べる", "たべる");
    assert_eq!(parts[0].text, "食");
    assert_eq!(parts[0].rt.as_deref(), Some("た"));
    assert_eq!(parts[1].rt, None);
    assert_eq!(to_hiragana("ネコ"), "ねこ");
    assert_eq!(numbered_to_marks("ni3 hao3"), "nǐ hǎo");
}

#[test]
fn every_app_saves_a_word_under_its_headword() {
    let w = words();
    let h = |lang: &str, t: &str| w.headword(lang, t, None);
    // Forms go to their word; a form that is a word of its own stays.
    assert_eq!(h("fr", "freinent").as_deref(), Some("freiner"));
    assert_eq!(h("fr", "gare").as_deref(), Some("gare"));
    // Elision and a sentence's capital go; the dictionary's spelling stays.
    assert_eq!(h("fr", "l'ardoise").as_deref(), Some("ardoise"));
    assert_eq!(h("fr", "s’attardent").as_deref(), None, "a form the dictionary doesn't have");
    assert_eq!(h("fr", "s'freinent").as_deref(), Some("s'freiner"), "a reflexive keeps its s'");
    assert_eq!(h("fr-CA", "Maison").as_deref(), Some("maison"));
    // A phrase: its first word's headword, then the rest.
    assert_eq!(h("fr", "découlent  de").as_deref(), Some("découler de"));
    // Nothing to save: numbers, names, fragments.
    assert_eq!(h("fr", "2024"), None);
    assert_eq!(h("fr", "Trump"), None);
    assert_eq!(h("fr", "mais0n"), None);
    assert_eq!(h("fr", "fre"), None);
    // German nouns keep their capital, whoever lower-cased them; a verb
    // spelled like a noun stays the verb.
    assert_eq!(h("de", "wasser").as_deref(), Some("Wasser"));
    assert_eq!(h("de", "urteil").as_deref(), Some("Urteil"));
    assert_eq!(h("de", "spur").as_deref(), Some("Spur"));
    assert_eq!(h("de", "fressen").as_deref(), Some("fressen"));
    assert_eq!(h("de", "Fressen").as_deref(), Some("Fressen"));
    // No dictionary for the language yet: as it is.
    assert_eq!(h("sv", " hej ").as_deref(), Some("hej"));
    // Japanese: the analyser's base form, as the apps pass it.
    assert_eq!(w.headword("ja", "食べた", Some("食べる")).as_deref(), Some("食べる"));
    assert_eq!(h("ja", "猫").as_deref(), Some("猫"));
    // Chinese words don't change.
    assert_eq!(h("zh", "喜欢").as_deref(), Some("喜欢"));
}
