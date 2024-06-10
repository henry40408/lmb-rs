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
pub use schedule::*;
pub use store::*;

mod check;
mod error;
mod eval;
mod example;
mod lua_binding;
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

/// Options to print script.
pub struct PrintOptions {
    /// No colors <https://no-color.org/>
    pub no_color: bool,
    /// Theme
    pub theme: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::io::empty;

    use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};
    use serde_json::json;

    use crate::{EvaluationBuilder, LmbStateKey, LmbStore, MIGRATIONS};

    #[test]
    fn lua_doc() {
        let value = include_str!("../docs/lua.md");
        let parser = Parser::new(value);

        let blocks = {
            let mut blocks = vec![];
            let mut text = String::new();
            let mut is_code = false;
            for event in parser {
                match event {
                    Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(l))) => {
                        if l.to_string() == "lua" {
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
            let store = LmbStore::default();
            let e = EvaluationBuilder::new(&block, empty()).store(store).build();
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
        let _ = LmbStateKey::from("key");
    }
}
