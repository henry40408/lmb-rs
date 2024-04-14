use thiserror::Error;

#[derive(Debug, Error)]
pub enum LamError {
    #[error("lua error: {0}")]
    Lua(#[from] mlua::Error),
    #[error("RMP decode error: {0}")]
    RMPDecode(#[from] rmp_serde::decode::Error),
    #[error("RMP encode error: {0}")]
    RMPEncode(#[from] rmp_serde::encode::Error),
    #[error("sqlite error: {0}")]
    SQLite(#[from] rusqlite::Error),
}
