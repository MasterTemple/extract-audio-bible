//! What can go wrong, by kind, so callers (and other languages) can tell missing files from bad
//! input from audio they can't read.

/// What kind of failure an [`Error`] is
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// A file, book, chapter, or source that isn't there
    NotFound,
    /// Reading or writing failed
    Io,
    /// Input that can't be used: a transcript that isn't JSON, a project without a folder to
    /// export to, an empty Bible
    Invalid,
    /// Audio that can't be decoded or encoded
    Audio,
    /// Audio that couldn't be downloaded
    Network,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("Bible text: {0}")]
    Text(#[from] logoxide_text::TextError),
    #[error("Audio: {0}")]
    Audio(String),
    #[error("Tags: {0}")]
    Tag(#[from] id3::Error),
    #[error("Downloading {url}: {message}")]
    Download { url: String, message: String },
    /// A project's readings database (`readings.sqlite`) that couldn't be read or written
    #[cfg(feature = "sqlite")]
    #[error("Readings database: {0}")]
    Database(#[from] rusqlite::Error),
}

impl Error {
    pub fn invalid(message: impl Into<String>) -> Self {
        Error::Invalid(message.into())
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Error::NotFound(message.into())
    }

    pub fn kind(&self) -> ErrorKind {
        match self {
            Error::NotFound(_) => ErrorKind::NotFound,
            Error::Io(e) if e.kind() == std::io::ErrorKind::NotFound => ErrorKind::NotFound,
            Error::Io(_) => ErrorKind::Io,
            Error::Text(logoxide_text::TextError::NotFound(_)) => ErrorKind::NotFound,
            Error::Text(logoxide_text::TextError::Files(_)) => ErrorKind::Io,
            Error::Invalid(_) | Error::Json(_) | Error::Text(_) => ErrorKind::Invalid,
            Error::Audio(_) | Error::Tag(_) => ErrorKind::Audio,
            Error::Download { .. } => ErrorKind::Network,
            #[cfg(feature = "sqlite")]
            Error::Database(_) => ErrorKind::Io,
        }
    }
}

impl From<symphonia::core::errors::Error> for Error {
    fn from(error: symphonia::core::errors::Error) -> Self {
        match error {
            symphonia::core::errors::Error::IoError(e) => Error::Io(e),
            e => Error::Audio(e.to_string()),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bible_text_errors_keep_their_kind() {
        use logoxide_text::TextError;
        assert_eq!(
            Error::from(TextError::NotFound("JHN".into())).kind(),
            ErrorKind::NotFound
        );
        assert_eq!(
            Error::from(TextError::Files("text/JHN.usx: unreadable".into())).kind(),
            ErrorKind::Io
        );
        let book = TextError::Book {
            book: "JHN".into(),
            message: "no verses".into(),
        };
        assert_eq!(Error::from(book).kind(), ErrorKind::Invalid);
    }
}
