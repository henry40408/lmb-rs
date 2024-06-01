use mlua::prelude::*;
use thiserror::Error;

/// Lmb error.
#[derive(Debug, Error)]
pub enum LmbError {
    /// Error from [`bat`].
    #[error("bat error: {0}")]
    Bat(#[from] bat::error::Error),
    /// Error from database.
    #[error("sqlite error: {0}")]
    Database(#[from] rusqlite::Error),
    /// Error from database migration.
    #[error("migration error: {0}")]
    DatabaseMigration(#[from] rusqlite_migration::Error),
    /// Format error.
    #[error("format error: {0}")]
    Format(#[from] std::fmt::Error),
    /// Invalid key length for HMAC
    #[error("invalid length: {0}")]
    InvalidLength(#[from] crypto_common::InvalidLength),
    /// Error from Lua engine.
    #[error("lua error: {0}")]
    Lua(#[from] LuaError),
    /// Error when decoding store value from message pack.
    #[error("RMP decode error: {0}")]
    RMPDecode(#[from] rmp_serde::decode::Error),
    /// Error when encoding store value to message pack.
    #[error("RMP encode error: {0}")]
    RMPEncode(#[from] rmp_serde::encode::Error),
    /// Error from [`serde_json`].
    #[error("serde JSON error: {0}")]
    SerdeJSONError(#[from] serde_json::Error),
}
