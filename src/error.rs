use mlua::prelude::*;
use thiserror::Error;

/// Lam error.
#[derive(Debug, Error)]
pub enum LamError {
    /// Error from database.
    #[error("sqlite error: {0}")]
    Database(#[from] rusqlite::Error),
    /// Error from database migration.
    #[error("migration error: {0}")]
    DatabaseMigration(#[from] rusqlite_migration::Error),
    /// Error from Lua engine.
    #[error("lua error: {0}")]
    Lua(#[from] LuaError),
    /// Error when decoding store value from message pack.
    #[error("RMP decode error: {0}")]
    RMPDecode(#[from] rmp_serde::decode::Error),
    /// Error when encoding store value to message pack.
    #[error("RMP encode error: {0}")]
    RMPEncode(#[from] rmp_serde::encode::Error),
    /// Invalid key length for HMAC
    #[error("invalid length: {0}")]
    InvalidLength(#[from] crypto_common::InvalidLength),
}
