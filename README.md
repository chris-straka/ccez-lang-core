# ccez-lang-core

The language-learning core the owner's apps share, in Rust: ccez-keys
(the Android keyboard) first, ccez-tube and ccez-lang later. Pulled out
of ccez-tube's core; nothing in it touches YouTube.

- **Words** (`words`, `dict`): the offline dictionaries ccez-lang builds
  (Wiktionary via kaikki, JMdict, CC-CEDICT), packed on the device and
  read a block at a time; a line cut into words with readings laid over
  them (furigana and pitch accent for Japanese, pinyin for Chinese); a
  word's entry with its meanings, dictionary form, gender and article.
  `romaja`: Korean as written and as it sounds.
- **Statuses** (`status`): Learning / Known / New for every word, kept
  on the device and synced with ccez-words (the owner's server; its
  address is the app's setting). Newest change wins per word.
- **AI** (`ai`, `correction`): the providers (Muse by default, the
  learner's own key), the prompts the apps share, a streaming
  OpenAI-compatible client, and ccez-llm's corrections (a correction
  block and a word diff).

## Use

    [dependencies]
    ccez-lang-core = { git = "https://github.com/chris-straka/ccez-lang-core", tag = "v0.1.0" }

Features: `net` (default: sync and the AI client, over rustls),
`ja` (Japanese words and furigana with lindera; IPAdic's files are read
from `root/ipadic` at run time, not built in), `uniffi` (UniFFI
derives on the records, for an app that hands them to Kotlin).

    let words = Words::new(dir);                       // packs live here
    words.install("fr", Path::new("fr.tsv.gz"))?;      // once, off the main thread
    let line = words.tokens("ja", "猫が好きです");      // words + furigana
    let entry = words.lookup("fr", "allons", None);    // → aller, "to go"

    let statuses = Statuses::new(dir);
    statuses.set("fr", "maison", Status::Learning);
    let came = statuses.sync(server, "ccez-keys:phone").await?;

    let m = correction::correction_messages("fr", typed, "en", Some(replying_to));
    let answer = ai::ask(p.url, key, p.model, &m, Some("minimal"), |so_far| {}).await?;
    let fix = correction::parse_correction(typed, &answer);   // text, why, diff

## Tests

    cargo test
    CCEZ_LANG_ASSETS=<folder with ja.tsv.gz, ja-pitch.tsv.gz, ipadic.zip> cargo test --release --features ja

Fixtures and examples are French, Japanese and Chinese. Android:
`cargo ndk -t arm64-v8a --platform 26 build --release`.

MIT (LICENSE). Data credits: CREDITS.md.
