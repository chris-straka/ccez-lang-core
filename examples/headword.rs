//! `cargo run --example headword -- <dictionary dir> <lang> <word>…`: each
//! word's headword (the form apps save it under), or "-" for none. With
//! "-" for the words, reads "lang<TAB>word" lines from stdin.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let w = ccez_lang_core::Words::new(&args[0]);
    let show = |lang: &str, word: &str| println!("{lang}\t{word}\t{}", w.headword(lang, word, None).unwrap_or_else(|| "-".into()));
    if args.get(1).map(String::as_str) == Some("-") {
        for line in std::io::stdin().lines().map_while(Result::ok) {
            if let Some((lang, word)) = line.split_once('\t') {
                show(lang, word);
            }
        }
    } else {
        for word in &args[2..] {
            show(&args[1], word);
        }
    }
}
