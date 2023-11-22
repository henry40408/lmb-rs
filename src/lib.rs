use std::{
    cell::RefCell,
    io::{BufRead, BufReader, Read},
    time::{Duration, Instant},
};

use mlua::{Lua, Table, ThreadStatus, Value, VmState};
use thiserror::Error;

const DEFAULT_TIMEOUT: u64 = 30;
const K_LOADED: &str = "_LOADED";

#[derive(Debug, Error)]
pub enum LamError {
    #[error("lua error: {0}")]
    Lua(#[from] mlua::Error),
}

type LamResult<T> = Result<T, LamError>;

pub struct Evaluation<R: Read> {
    pub input: RefCell<BufReader<R>>,
    pub script: String,
    pub timeout: Option<u64>,
}

impl<R: Read> Evaluation<R> {
    pub fn new(c: EvaluationConfig<R>) -> Evaluation<R> {
        Evaluation {
            input: RefCell::new(BufReader::new(c.input)),
            script: c.script,
            timeout: c.timeout,
        }
    }
}

pub struct EvaluationConfig<R: Read> {
    pub input: R,
    pub script: String,
    pub timeout: Option<u64>,
}

#[derive(Debug)]
pub struct EvaluationResult {
    pub duration: Duration,
    pub result: String,
}

pub fn evaluate<R: Read>(e: &mut Evaluation<R>) -> LamResult<EvaluationResult> {
    let start = Instant::now();
    let timeout = e.timeout.unwrap_or(DEFAULT_TIMEOUT) as f32;

    let vm = Lua::new();
    vm.sandbox(true)?;
    vm.set_interrupt(move |_| {
        if start.elapsed().as_secs_f32() > timeout {
            return Ok(VmState::Yield);
        }
        Ok(VmState::Continue)
    });

    let r = vm.scope(|scope| {
        let loaded = vm.named_registry_value::<Table<'_>>(K_LOADED)?;

        let m = vm.create_table()?;
        m.set("_VERSION", env!("CARGO_PKG_VERSION"))?;

        let read_fn = scope.create_function(|_, f: Value<'_>| {
            if let Some(f) = f.as_str() {
                if f.starts_with("*a") {
                    // accepts *a or *all
                    let mut buf = Vec::new();
                    e.input.borrow_mut().read_to_end(&mut buf)?;
                    let s = vm.create_string(String::from_utf8(buf).unwrap_or_default())?;
                    return Ok(Value::String(s));
                }
                if f.starts_with("*l") {
                    // accepts *l or *line
                    let mut r = e.input.borrow_mut();
                    let mut buf = String::new();
                    r.read_line(&mut buf)?;
                    let s = vm.create_string(buf)?;
                    return Ok(Value::String(s));
                }
                if f.starts_with("*n") {
                    // accepts *n or *number
                    let mut buf = String::new();
                    e.input.borrow_mut().read_to_string(&mut buf)?;
                    return Ok(buf.parse::<f64>().map(Value::Number).unwrap_or(Value::Nil));
                }
            }

            #[allow(clippy::unused_io_amount)]
            if let Some(i) = f.as_usize() {
                let mut buf = vec![0; i];
                let count = e.input.borrow_mut().read(&mut buf)?;
                buf.truncate(count);
                let s = vm.create_string(String::from_utf8(buf).unwrap_or_default())?;
                return Ok(Value::String(s));
            }

            let s = format!("unexpected format {f:?}");
            Err(mlua::Error::RuntimeError(s))
        })?;
        m.set("read", read_fn)?;

        let read_unicode_fn = scope.create_function(|_, i: usize| {
            let mut expected_read = i;
            let mut buf = Vec::new();
            let mut byte_buf = vec![0; 1];
            loop {
                if expected_read == 0 {
                    return Ok(String::from_utf8(buf).unwrap_or_default());
                }
                let read_bytes = e.input.borrow_mut().read(&mut byte_buf)?;
                // caveat: buffer is not empty when no bytes are read
                if read_bytes > 0 {
                    buf.extend_from_slice(&byte_buf);
                }
                if read_bytes == 0 {
                    return Ok(String::from_utf8(buf).unwrap_or_default());
                }
                if std::str::from_utf8(&buf).is_ok() {
                    expected_read -= 1;
                }
            }
        })?;
        m.set("read_unicode", read_unicode_fn)?;

        loaded.set("@lam", m)?;
        vm.set_named_registry_value(K_LOADED, loaded)?;

        let co = vm.create_thread(vm.load(&e.script).into_function()?)?;
        loop {
            let res = co.resume::<_, Option<String>>(())?;
            if co.status() != ThreadStatus::Resumable || start.elapsed().as_secs_f32() > timeout {
                let r = EvaluationResult {
                    duration: start.elapsed(),
                    result: res.unwrap_or(String::new()),
                };
                return Ok(r);
            }
        }
    })?;
    Ok(r)
}

#[cfg(test)]
mod test {
    use std::io::Cursor;

    use crate::{evaluate, Evaluation, EvaluationConfig};

    const TIMEOUT_THRESHOLD: f32 = 0.01;

    #[test]
    fn test_evaluate_infinite_loop() {
        let timeout = 1;

        let mut e = Evaluation::new(EvaluationConfig {
            input: Cursor::new(""),
            script: r#"while true do end"#.to_string(),
            timeout: Some(timeout),
        });
        let res = evaluate(&mut e).unwrap();
        assert_eq!("", res.result);

        let secs = res.duration.as_secs_f32();
        let to = timeout as f32;
        assert!((secs - to) / to < TIMEOUT_THRESHOLD, "timed out {}s", secs);
    }

    #[test]
    fn test_read_all() {
        let input = "lam";
        let mut e = Evaluation::new(EvaluationConfig {
            input: Cursor::new(input),
            script: r#"local m = require('@lam'); return m.read('*a')"#.to_string(),
            timeout: None,
        });
        let res = evaluate(&mut e).unwrap();
        assert_eq!(input, res.result);
    }

    #[test]
    fn test_read_partial_input() {
        let input = "lam";
        let mut e = Evaluation::new(EvaluationConfig {
            input: Cursor::new(input),
            script: r#"local m = require('@lam'); return m.read(1)"#.to_string(),
            timeout: None,
        });
        let res = evaluate(&mut e).unwrap();
        assert_eq!("l", res.result);
    }

    #[test]
    fn test_read_more_than_input() {
        let input = "l";
        let mut e = Evaluation::new(EvaluationConfig {
            input: Cursor::new(input),
            script: r#"local m = require('@lam'); return m.read(3)"#.to_string(),
            timeout: None,
        });
        let res = evaluate(&mut e).unwrap();
        assert_eq!("l", res.result);
    }

    #[test]
    fn test_read_unicode() {
        let input = "你好";
        let mut e = Evaluation::new(EvaluationConfig {
            input: Cursor::new(input),
            script: r#"local m = require('@lam'); return m.read_unicode(1)"#.to_string(),
            timeout: None,
        });
        let res = evaluate(&mut e).unwrap();
        assert_eq!("你", res.result);
    }

    #[test]
    fn test_read_line() {
        let input = "foo\nbar";
        let mut e = Evaluation::new(EvaluationConfig {
            input: Cursor::new(input),
            script: r#"local m = require('@lam'); m.read('*l'); return m.read('*l')"#.to_string(),
            timeout: None,
        });
        let res = evaluate(&mut e).unwrap();
        assert_eq!("bar", res.result);
    }

    #[test]
    fn test_read_number() {
        let input = "3.1415926";
        let mut e = Evaluation::new(EvaluationConfig {
            input: Cursor::new(input),
            script: r#"local m = require('@lam'); return m.read('*n')"#.to_string(),
            timeout: None,
        });
        let res = evaluate(&mut e).unwrap();
        assert_eq!("3.1415926", res.result);
    }

    #[test]
    fn test_read_integer() {
        let input = "3";
        let mut e = Evaluation::new(EvaluationConfig {
            input: Cursor::new(input),
            script: r#"local m = require('@lam'); return m.read('*n')"#.to_string(),
            timeout: None,
        });
        let res = evaluate(&mut e).unwrap();
        assert_eq!("3", res.result);
    }

    #[test]
    fn test_multiple_evaluation() {
        let input = "2";
        let mut e = Evaluation::new(EvaluationConfig {
            input: Cursor::new(input),
            script: r#"local m = require('@lam'); return tonumber(m.read('*n'))*2"#.to_string(),
            timeout: None,
        });
        let res = evaluate(&mut e).unwrap();
        assert_eq!("4", res.result);

        let input = res.result;
        let mut e = Evaluation::new(EvaluationConfig {
            input: Cursor::new(input),
            script: r#"local m = require('@lam'); return tonumber(m.read('*n'))+3"#.to_string(),
            timeout: None,
        });
        let res = evaluate(&mut e).unwrap();
        assert_eq!("7", res.result);
    }
}
