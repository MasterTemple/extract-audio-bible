use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("Bible text: {0}")]
    Text(#[from] logoxide_text::TextError),
    #[error("Audio: {0}")]
    Audio(#[from] symphonia::core::errors::Error),
    #[error("Tags: {0}")]
    Tag(#[from] id3::Error),
    #[error(transparent)]
    Tauri(#[from] tauri::Error),
}

impl AppError {
    pub fn msg(message: impl Into<String>) -> Self {
        AppError::Message(message.into())
    }
}

/// Commands reject with the error's message, which the UI shows as is
impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
