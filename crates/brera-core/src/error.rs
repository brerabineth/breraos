use thiserror::Error;

/// Every failure mode brera-core can report.
///
/// The shell maps these to user-visible strings; the web layer only ever
/// sees the `kind` tag, never internals.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CoreError {
    #[error("mnemonic rejected: {0}")]
    Mnemonic(String),

    #[error("vault is locked")]
    VaultLocked,

    #[error("vault is already unlocked")]
    VaultUnlocked,

    #[error("derivation failed at path {path}: {note}")]
    Derivation { path: String, note: String },

    #[error("crypto channel failure: {0}")]
    Channel(String),

    #[error("transport unavailable: {0}")]
    Transport(String),

    #[error("proxy failure: {0}")]
    Proxy(String),

    #[error("signer channel dropped")]
    SignerClosed,

    #[error("payload exceeds limit ({limit} bytes)")]
    PayloadTooLarge { limit: usize },

    #[error("malformed frame: {0}")]
    Frame(String),

    #[error("io failure: {0}")]
    Io(String),

    #[error("{0}")]
    Other(String),
}

impl From<std::io::Error> for CoreError {
    fn from(e: std::io::Error) -> Self {
        CoreError::Io(e.to_string())
    }
}

impl From<chacha20poly1305::aead::Error> for CoreError {
    fn from(_: chacha20poly1305::aead::Error) -> Self {
        // AEAD never explains why; a bad tag is a bad tag.
        CoreError::Channel("authentication tag mismatch".into())
    }
}
