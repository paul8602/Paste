use thiserror::Error;
use serde::Serialize;

#[derive(Debug, Error)]
pub enum PasteError {
    #[error("history store lock poisoned")]
    LockPoisoned,

    #[error(transparent)]
    Store(#[from] rusqlite::Error),

    #[error("{0}")]
    Clipboard(String),

    #[error("failed to open Accessibility settings: {0}")]
    OpenSettings(String),

    #[error("failed to resolve app data dir: {0}")]
    AppDataDir(String),

    #[error("failed to open history store: {0}")]
    HistoryStore(String),

    #[error("failed to register global shortcut: {0}")]
    GlobalShortcut(String),
}

impl Serialize for PasteError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
