pub use error::LamError;
pub use eval::{evaluate, EvalBuilder};
use parking_lot::Mutex;
use std::{io::BufReader, sync::Arc};
pub use store::LamStore;
pub use value::LamValue;

mod error;
mod eval;
mod lua_lam;
mod store;
mod value;

pub type LamInput<R> = Arc<Mutex<BufReader<R>>>;
pub type LamResult<T> = Result<T, LamError>;
