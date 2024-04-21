use std::io::BufReader;

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

pub type LamInput<R> = BufReader<R>;
pub type LamResult<T> = Result<T, LamError>;
