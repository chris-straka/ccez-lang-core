//! What can go wrong, in words an app can show.

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Error))]
pub enum LangError {
    #[error("no dictionary for {lang}")]
    NoDictionary { lang: String },
    #[error("couldn't write the dictionary: {reason}")]
    Storage { reason: String },
    #[error("no connection")]
    Network,
    #[error("the server took too long")]
    Timeout,
    #[error("the server's answer made no sense")]
    BadResponse,
    #[error("{reason}")]
    Ai { reason: String },
}

pub type Result<T> = std::result::Result<T, LangError>;
