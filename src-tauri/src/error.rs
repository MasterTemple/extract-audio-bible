use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Core(#[from] extract_audio_bible::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Tauri(#[from] tauri::Error),
}

impl AppError {
    pub fn msg(message: impl Into<String>) -> Self {
        AppError::Message(message.into())
    }

    /// What kind of failure it is, as the UI tells them apart: `notFound`, `io`, `invalid`,
    /// `audio`, `network`, or `app` (the app itself failed)
    pub fn kind(&self) -> &'static str {
        use extract_audio_bible::ErrorKind as K;
        match self {
            AppError::Core(e) => match e.kind() {
                K::NotFound => "notFound",
                K::Io => "io",
                K::Audio => "audio",
                K::Network => "network",
                // `Invalid`, and any kind a newer library adds
                _ => "invalid",
            },
            AppError::Io(e) if e.kind() == std::io::ErrorKind::NotFound => "notFound",
            AppError::Io(_) => "io",
            AppError::Message(_) | AppError::Json(_) => "invalid",
            AppError::Tauri(_) => "app",
        }
    }
}

/// Commands reject with `{kind, message}`: the message the UI shows as is, and its kind
impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut error = serializer.serialize_struct("AppError", 2)?;
        error.serialize_field("kind", self.kind())?;
        error.serialize_field("message", &self.to_string())?;
        error.end()
    }
}

/// A file that couldn't be read or written, named, keeping its kind
pub fn file_error(path: impl std::fmt::Display, e: std::io::Error) -> AppError {
    AppError::Io(std::io::Error::new(e.kind(), format!("{path}: {e}")))
}

pub type Result<T> = std::result::Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_kind_and_message() {
        let missing = file_error("bible.json", std::io::Error::from(std::io::ErrorKind::NotFound));
        let json = serde_json::to_value(&missing).unwrap();
        assert_eq!(json["kind"], "notFound");
        assert!(json["message"].as_str().unwrap().starts_with("bible.json: "));
        let core = AppError::from(extract_audio_bible::Error::invalid("No verses were found"));
        assert_eq!(
            serde_json::to_value(&core).unwrap(),
            serde_json::json!({"kind": "invalid", "message": "No verses were found"})
        );
    }
}
