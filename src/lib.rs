#![deny(missing_docs)]

//! A Lua function runner.

use dashmap::DashMap;
use parking_lot::Mutex;
use std::sync::Arc;

pub use check::*;
pub use error::*;
pub use eval::*;
pub use example::*;
pub use lua_lam::*;
pub use printer::*;
pub use store::*;
pub use value::*;

mod check;
mod error;
mod eval;
mod example;
mod lua_lam;
mod printer;
mod store;
mod value;

/// Function input.
#[cfg(not(tarpaulin_include))]
pub type LamInput<R> = Arc<Mutex<R>>;

/// Generic result type of Lam.
#[cfg(not(tarpaulin_include))]
pub type LamResult<T> = Result<T, LamError>;

/// State key
#[derive(Hash, PartialEq, Eq)]
pub enum LamStateKey {
    /// Reserved key for HTTP request object
    Request,
    /// Plain string key
    String(String),
}

impl<S> From<S> for LamStateKey
where
    S: AsRef<str>,
{
    fn from(value: S) -> Self {
        Self::String(value.as_ref().to_string())
    }
}

/// State of each evaluation.
#[cfg(not(tarpaulin_include))]
pub type LamState = DashMap<LamStateKey, LamValue>;

#[cfg(test)]
mod tests {
    use crate::LamStateKey;

    #[test]
    fn state_key_from_str() {
        let _ = LamStateKey::from("key");
    }
}
