use std::fmt::Display;

use rlua::Lua;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LamError {
    #[error("lua error: {0}")]
    Lua(#[from] rlua::Error),
}

type LamResult<T> = Result<T, LamError>;

pub fn evaluate<D: Display>(script: D) -> LamResult<String> {
    let state = Lua::new();

    let script = script.to_string();
    let res = state.context(|ctx| {
        let res = ctx
            .load(&script)
            .set_name("eval")?
            .eval::<Option<String>>()?;
        Ok::<_, LamError>(res.unwrap_or(String::new()))
    })?;

    Ok(res)
}
