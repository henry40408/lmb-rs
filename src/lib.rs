#![deny(missing_docs)]

//! A Lua function runner.

use dashmap::DashMap;
use include_dir::{include_dir, Dir};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use rusqlite_migration::Migrations;
use std::{io::BufReader, sync::Arc, time::Duration};

pub use check::*;
pub use error::*;
pub use eval::*;
pub use example::*;
pub use lua_binding::*;
pub use printer::*;
pub use schedule::*;
pub use store::*;

mod check;
mod error;
mod eval;
mod example;
mod lua_binding;
mod printer;
mod schedule;
mod store;

/// Default timeout for evaluation in seconds.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

static MIGRATIONS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/migrations");

static MIGRATIONS: Lazy<Migrations<'static>> = Lazy::new(|| {
    Migrations::from_directory(&MIGRATIONS_DIR)
        .expect("failed to load migrations from the directory")
});

/// Function input.
pub type LmbInput<R> = Arc<Mutex<BufReader<R>>>;

/// Generic result type of Lmb.
pub type LmbResult<T> = Result<T, LmbError>;

/// State key.
#[derive(Hash, PartialEq, Eq)]
pub enum LmbStateKey {
    /// HTTP request object
    Request,
    /// HTTP response object
    Response,
    /// Plain string key
    String(String),
}

impl<S> From<S> for LmbStateKey
where
    S: AsRef<str>,
{
    fn from(value: S) -> Self {
        Self::String(value.as_ref().to_string())
    }
}

/// State of each evaluation.
pub type LmbState = DashMap<LmbStateKey, serde_json::Value>;

#[cfg(test)]
mod tests {
    use crate::{LmbStateKey, MIGRATIONS};

    #[test]
    fn migrations() {
        MIGRATIONS.validate().unwrap();
    }

    #[test]
    fn state_key_from_str() {
        let _ = LmbStateKey::from("key");
    }
}
