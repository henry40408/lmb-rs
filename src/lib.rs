#![deny(missing_debug_implementations, missing_docs)]

//! A Lua function runner.

use bon::Builder;
use dashmap::DashMap;
use include_dir::{include_dir, Dir};
use parking_lot::Mutex;
use rusqlite_migration::Migrations;
use std::{
    fmt::Display,
    io::BufReader,
    result::Result as StdResult,
    sync::{Arc, LazyLock},
    time::Duration,
};

pub use check::*;
pub use error::*;
pub use eval::*;
pub use example::*;
pub use guide::*;
pub use lua_binding::*;
pub use schedule::*;
pub use store::*;

mod check;
mod error;
mod eval;
mod example;
mod guide;
mod lua_binding;
mod schedule;
mod store;

/// Default timeout for evaluation in seconds.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Directory containing migration files.
static MIGRATIONS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/migrations");

/// Migrations for the `SQLite` database.
static MIGRATIONS: LazyLock<Migrations<'static>> = LazyLock::new(|| {
    Migrations::from_directory(&MIGRATIONS_DIR)
        .expect("failed to load migrations from the directory")
});

/// Function input, wrapped in an Arc and Mutex for thread safety.
pub type Input<R> = Arc<Mutex<BufReader<R>>>;

/// Generic result type for the function runner.
pub type Result<T> = StdResult<T, Error>;

/// Enum representing different state keys.
#[derive(Debug, Eq, Hash, PartialEq)]
pub enum StateKey {
    /// HTTP request object
    Request,
    /// HTTP response object
    Response,
    /// Plain string key
    String(String),
}

impl<S> From<S> for StateKey
where
    S: Display,
{
    /// Converts a type that can be referenced as a string into a [`StateKey`].
    fn from(value: S) -> Self {
        Self::String(value.to_string())
    }
}

/// State of each evaluation, using a [`dashmap::DashMap`].
pub type State = DashMap<StateKey, serde_json::Value>;

/// Options for printing scripts.
#[derive(Builder, Debug)]
pub struct PrintOptions {
    /// Disable colors [`https://no-colors.org`].
    no_color: bool,
    /// Theme.
    theme: Option<String>,
}

#[cfg(test)]
mod tests {
    use crate::{build_evaluation, StateKey, Store, MIGRATIONS};
    use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
    use serde_json::json;
    use std::io::empty;

    #[test]
    fn test_evaluation() {
        let markdown = include_str!("../guides/lua.md");
        let blocks = {
            let mut blocks = Vec::new();
            let parser = Parser::new_ext(markdown, Options::all());
            let mut is_code = false;
            let mut text = String::new();

            for event in parser {
                match event {
                    Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(ref lang))) => {
                        if lang.to_string() == "lua" {
                            is_code = true;
                        }
                    }
                    Event::Text(t) => {
                        if is_code {
                            text.push_str(&t);
                        }
                    }
                    Event::End(TagEnd::CodeBlock) => {
                        if is_code {
                            blocks.push(text.clone());
                            text.clear();
                            is_code = false;
                        }
                    }
                    _ => {}
                }
            }
            blocks
        };

        let mut server = mockito::Server::new();

        let headers_mock = server
            .mock("GET", "/headers")
            .with_status(200)
            .match_header("I-Am", "A teapot")
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::to_string(&json!({ "headers": { "I-Am": "A teapot" } })).unwrap(),
            )
            .create();

        let post_mock = server
            .mock("POST", "/post")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::to_string(
                    &json!({ "data": serde_json::to_string(&json!({ "foo": "bar" })).unwrap() }),
                )
                .unwrap(),
            )
            .create();

        for block in blocks {
            let block = block.replace("https://httpbin.org", &server.url());
            let store = Store::default();
            let e = build_evaluation(&block, empty())
                .store(store)
                .call()
                .unwrap();
            e.evaluate().unwrap();
        }

        post_mock.assert();
        headers_mock.assert();
    }

    #[test]
    fn migrations() {
        MIGRATIONS.validate().unwrap();
    }

    #[test]
    fn state_key_from_str() {
        let _ = StateKey::from("key");
    }
}
