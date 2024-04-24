#![deny(missing_docs)]

//! A Lua function runner.

use parking_lot::Mutex;
use std::{io::BufReader, sync::Arc};

pub use check::*;
pub use error::*;
pub use eval::*;
pub use lua_lam::*;
pub use store::*;
pub use value::*;

mod check;
mod error;
mod eval;
mod lua_lam;
mod store;
mod value;

/// Function input.
pub type LamInput<R> = Arc<Mutex<BufReader<R>>>;

/// Generic result type of Lam.
pub type LamResult<T> = Result<T, LamError>;
