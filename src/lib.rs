#![deny(missing_docs)]

//! A Lua function runner.

use dashmap::DashMap;
use include_dir::{include_dir, Dir};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use rusqlite_migration::Migrations;
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

static MIGRATIONS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/migrations");

static MIGRATIONS: Lazy<Migrations<'static>> = Lazy::new(|| {
    Migrations::from_directory(&MIGRATIONS_DIR)
        .expect("failed to load migrations from the directory")
});

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
    use crate::{LamStateKey, MIGRATIONS};

    #[test]
    fn migrations() {
        MIGRATIONS.validate().unwrap();
    }

    #[test]
    fn state_key_from_str() {
        let _ = LamStateKey::from("key");
    }
}
