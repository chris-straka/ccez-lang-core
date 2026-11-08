//! Japanese cut by lindera with IPAdic, with furigana and pitch accent,
//! on the real files. They're large and not in this repo: point
//! `CCEZ_LANG_ASSETS` at a folder with ccez-lang's `ja.tsv.gz`,
//! `ja-pitch.tsv.gz` and `ipadic.zip` (ccez-tube keeps them in
//! android/app/src/main/assets/dict). Without it these tests pass empty.
#![cfg(feature = "ja")]

use std::path::PathBuf;
use std::process::Command;

use ccez_lang_core::Words;

fn words() -> Option<Words> {
    let assets = PathBuf::from(std::env::var("CCEZ_LANG_ASSETS").ok()?);
    let root = std::env::temp_dir().join(format!("ccez-lang-ja-{}", std::process::id()));
    if !root.join("ipadic").exists() {
        std::fs::create_dir_all(&root).unwrap();
        let ok = Command::new("unzip").args(["-oq", &assets.join("ipadic.zip").to_string_lossy(), "-d"]).arg(&root).status().unwrap();
        assert!(ok.success());
        std::fs::rename(root.join("lindera-ipadic"), root.join("ipadic")).unwrap();
    }
    let w = Words::new(&root);
    w.install("ja", &assets.join("ja.tsv.gz")).unwrap();
    w.install_pitch(&assets.join("ja-pitch.tsv.gz")).unwrap();
    assert!(w.ready("ja"));
    Some(w)
}

#[test]
fn a_sentence_with_furigana_and_dictionary_forms() {
    let Some(w) = words() else { return };
    let t = w.tokens("ja", "明日は猫と寿司を食べました。");
    let cut: Vec<&str> = t.iter().filter(|t| t.word).map(|t| t.text.as_str()).collect();
    assert!(cut.contains(&"猫") && cut.contains(&"寿司"), "{cut:?}");
    let neko = t.iter().find(|t| t.text == "猫").unwrap();
    assert_eq!(neko.reading.as_deref(), Some("ねこ"));
    assert_eq!(neko.ruby[0].rt.as_deref(), Some("ねこ"));
    let tabe = t.iter().find(|t| t.text.starts_with('食')).unwrap();
    assert_eq!(tabe.lemma.as_deref(), Some("食べる"));
    assert_eq!(tabe.ruby[0].rt.as_deref(), Some("た"));
    assert_eq!(w.lookup("ja", &tabe.text, tabe.lemma.as_deref()).unwrap().senses[0].meanings[0], "to eat");
    // Pitch: 猫 (ねこ) is atamadaka, high on the first mora.
    assert_eq!(neko.accent, Some(1));
    assert_eq!(neko.ruby[0].pitch, [2, 0]);
    assert_eq!(t.iter().map(|t| t.text.as_str()).collect::<String>(), "明日は猫と寿司を食べました。");
}
