#![deny(missing_docs)]

//! A Lua function runner.

use dashmap::DashMap;
use parking_lot::Mutex;
use std::{io::BufReader, sync::Arc};

pub use check::*;
pub use error::*;
pub use eval::*;
pub use example::*;
pub use lua_lam::*;
pub use store::*;
pub use value::*;

mod check;
mod error;
mod eval;
mod example;
mod lua_lam;
mod store;
mod value;

/// Function input.
pub type LamInput<R> = Arc<Mutex<BufReader<R>>>;

/// Generic result type of Lam.
pub type LamResult<T> = Result<T, LamError>;

/// State key
#[derive(Hash, PartialEq, Eq)]
pub enum LamStateKey {
    /// Reserved key for HTTP request object
    Request,
    /// Plain string key
    String(String),
}

/// State of each evaluation.
pub type LamState = DashMap<LamStateKey, LamValue>;
