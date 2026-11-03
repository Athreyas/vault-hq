//! One error type for the whole core. It serializes to what the UI expects:
//! `{ kind, message, current? }` (SPEC section 10).

use serde::{Serialize, Serializer};
use ts_rs::TS;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    NotFound(String),
    /// A guarded write found the file changed since the writer read it.
    #[error("This note changed after you started editing.")]
    Conflict { current: Current },
    #[error("{0}")]
    Invalid(String),
    /// Refused on purpose: a blocked vault, a write outside a vault, an address that isn't local.
    #[error("{0}")]
    Denied(String),
    /// Something the app depends on is missing or down (the model server, a browser's file).
    #[error("{0}")]
    Unavailable(String),
    #[error("{0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// The text and hash on disk, returned with a conflict so the user can compare and choose.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Current {
    pub text: String,
    pub hash: String,
}

#[derive(Serialize, TS)]
#[ts(export, rename = "HqError")]
#[serde(rename_all = "camelCase")]
pub struct ErrorView<'a> {
    pub kind: &'a str,
    pub message: String,
    #[ts(optional)]
    pub current: Option<Current>,
}

impl Error {
    pub fn kind(&self) -> &'static str {
        match self {
            Error::NotFound(_) => "NotFound",
            Error::Conflict { .. } => "Conflict",
            Error::Invalid(_) => "Invalid",
            Error::Denied(_) => "Denied",
            Error::Unavailable(_) => "Unavailable",
            Error::Internal(_) => "Internal",
        }
    }
}

impl Serialize for Error {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let current = match self {
            Error::Conflict { current } => Some(current.clone()),
            _ => None,
        };
        ErrorView {
            kind: self.kind(),
            message: self.to_string(),
            current,
        }
        .serialize(s)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::NotFound => Error::NotFound(e.to_string()),
            std::io::ErrorKind::PermissionDenied => Error::Denied(e.to_string()),
            _ => Error::Internal(e.to_string()),
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Invalid(e.to_string())
    }
}
