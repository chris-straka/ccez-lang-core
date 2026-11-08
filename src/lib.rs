//! The language-learning core the owner's apps share (ccez-keys first;
//! ccez-tube and ccez-lang to follow), pulled out of ccez-tube's core:
//!
//! - [`words`]: the offline dictionaries ([`dict`] packs), lines cut into
//!   words with their readings (furigana with the `ja` feature, pinyin,
//!   Japanese pitch accent), and a word's entry (meanings, gender,
//!   article). [`romaja`]: Korean as written and as it sounds.
//! - [`sense`]: which dictionary meaning a translation uses.
//!
//! Features: `ja` for lindera, `uniffi` for UniFFI derives on the records.

pub mod dict;
mod error;
pub mod romaja;
pub mod sense;
pub mod words;

pub use error::{LangError, Result};
pub use lexicon;
pub use lexicon::base_lang;
pub use words::{Entry, RubyPart, Sense, Token, Words};

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!("ccez_lang_core");
