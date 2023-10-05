use std::time::Instant;

use mlua::{Lua, ThreadStatus, VmState};
use thiserror::Error;

const DEFAULT_TIMEOUT: u64 = 30;

#[derive(Debug, Error)]
pub enum LamError {
    #[error("lua error: {0}")]
    Lua(#[from] mlua::Error),
}

type LamResult<T> = Result<T, LamError>;

#[derive(Debug)]
pub struct Evaluation {
    pub script: String,
    pub timeout: Option<u64>,
}

pub fn evaluate(e: &Evaluation) -> LamResult<String> {
    let start = Instant::now();
    let timeout = e.timeout.unwrap_or(DEFAULT_TIMEOUT) as f32;

    let vm = Lua::new();
    vm.set_interrupt(move |_| {
        if start.elapsed().as_secs_f32() > timeout {
            return Ok(VmState::Yield);
        }
        Ok(VmState::Continue)
    });
    let co = vm.create_thread(vm.load(&e.script).into_function()?)?;
    loop {
        let res = co.resume::<_, Option<String>>(())?;
        if co.status() != ThreadStatus::Resumable || start.elapsed().as_secs_f32() > timeout {
            return Ok(res.unwrap_or(String::new()));
        }
    }
}

#[cfg(test)]
mod test {
    use std::time::Instant;

    use crate::{evaluate, Evaluation};

    #[test]
    fn test_evaluate_infinite_loop() {
        let timeout = 1;

        let start = Instant::now();
        let e = Evaluation {
            script: r#"while true do end"#.to_string(),
            timeout: Some(timeout),
        };
        let res = evaluate(&e).unwrap();
        assert_eq!("", res);

        let s = start.elapsed().as_secs_f32();
        let timeout = timeout as f32;
        assert!(s < timeout * 1.01, "timed out {}s", s);
    }
}
