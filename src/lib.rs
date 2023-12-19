use std::{
    cell::RefCell,
    io::{BufRead, BufReader, Read},
    time::{Duration, Instant},
};

use mlua::{Lua, Table, ThreadStatus, VmState};
use thiserror::Error;

const DEFAULT_TIMEOUT: u64 = 30;
const K_LOADED: &str = "_LOADED";

#[derive(Debug, Error)]
pub enum LamError {
    #[error("lua error: {0}")]
    Lua(#[from] mlua::Error),
}

type LamResult<T> = Result<T, LamError>;

pub struct Evaluation<R>
where
    R: Read,
{
    pub input: RefCell<BufReader<R>>,
    pub script: String,
    pub timeout: Option<u64>,
}

impl<R> Evaluation<R>
where
    R: Read,
{
    pub fn new(c: EvalConfig<R>) -> Self {
        Self {
            input: RefCell::new(BufReader::new(c.input)),
            script: c.script,
            timeout: Some(c.timeout),
        }
    }
}

pub struct EvalConfig<R>
where
    R: Read,
{
    pub input: R,
    pub script: String,
    pub timeout: u64,
}

pub struct EvalConfigBuilder<R>
where
    R: Read,
{
    pub input: R,
    pub script: String,
    pub timeout: Option<u64>,
}

impl<R> EvalConfigBuilder<R>
where
    R: Read,
{
    pub fn new<S: AsRef<str>>(input: R, script: S) -> Self {
        Self {
            input,
            script: script.as_ref().to_string(),
            timeout: None,
        }
    }

    pub fn set_timeout(mut self, timeout: u64) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn build(self) -> EvalConfig<R> {
        EvalConfig {
            input: self.input,
            script: self.script,
            timeout: self.timeout.unwrap_or(60),
        }
    }
}

#[derive(Debug)]
pub struct EvalResult {
    pub duration: Duration,
    pub result: String,
}

pub fn evaluate<R>(e: &mut Evaluation<R>) -> LamResult<EvalResult>
where
    R: Read,
{
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
        let m = vm.create_table()?;
        m.set("_VERSION", env!("CARGO_PKG_VERSION"))?;

        let read_fn = scope.create_function(|_, f: mlua::Value<'_>| {
            if let Some(f) = f.as_str() {
                if f.starts_with("*a") {
                    // accepts *a or *all
                    let mut buf = Vec::new();
                    e.input.borrow_mut().read_to_end(&mut buf)?;
                    let s = vm.create_string(String::from_utf8(buf).unwrap_or_default())?;
                    return Ok(mlua::Value::String(s));
                }
                if f.starts_with("*l") {
                    // accepts *l or *line
                    let mut r = e.input.borrow_mut();
                    let mut buf = String::new();
                    r.read_line(&mut buf)?;
                    let s = vm.create_string(buf)?;
                    return Ok(mlua::Value::String(s));
                }
                if f.starts_with("*n") {
                    // accepts *n or *number
                    let mut buf = String::new();
                    e.input.borrow_mut().read_to_string(&mut buf)?;
                    return Ok(buf
                        .parse::<f64>()
                        .map(mlua::Value::Number)
                        .unwrap_or(mlua::Value::Nil));
                }
            }

            #[allow(clippy::unused_io_amount)]
            if let Some(i) = f.as_usize() {
                let mut buf = vec![0; i];
                let count = e.input.borrow_mut().read(&mut buf)?;
                buf.truncate(count);
                let s = vm.create_string(String::from_utf8(buf).unwrap_or_default())?;
                return Ok(mlua::Value::String(s));
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

        let loaded = vm.named_registry_value::<Table<'_>>(K_LOADED)?;
        loaded.set("@lam", m)?;
        vm.set_named_registry_value(K_LOADED, loaded)?;

        let co = vm.create_thread(vm.load(&e.script).into_function()?)?;
        loop {
            let res = co.resume::<_, Option<String>>(())?;
            if co.status() != ThreadStatus::Resumable || start.elapsed().as_secs_f32() > timeout {
                let r = EvalResult {
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

    use crate::{evaluate, EvalConfigBuilder, Evaluation};

    const TIMEOUT_THRESHOLD: f32 = 0.01;

    #[test]
    fn test_evaluate_infinite_loop() {
        let timeout = 1;

        let input: &[u8] = &[];
        let c = EvalConfigBuilder::new(input, r#"while true do end"#)
            .set_timeout(timeout)
            .build();
        let mut e = Evaluation::new(c);
        let res = evaluate(&mut e).unwrap();
        assert_eq!("", res.result);

        let secs = res.duration.as_secs_f32();
        let to = timeout as f32;
        assert!((secs - to) / to < TIMEOUT_THRESHOLD, "timed out {}s", secs);
    }

    #[test]
    fn test_read_all() {
        let input = "lam";
        let c = EvalConfigBuilder::new(
            Cursor::new(input),
            r#"local m = require('@lam'); return m.read('*a')"#,
        )
        .build();
        let mut e = Evaluation::new(c);
        let res = evaluate(&mut e).unwrap();
        assert_eq!(input, res.result);
    }

    #[test]
    fn test_read_partial_input() {
        let input = "lam";
        let c = EvalConfigBuilder::new(
            Cursor::new(input),
            r#"local m = require('@lam'); return m.read(1)"#,
        )
        .build();
        let mut e = Evaluation::new(c);
        let res = evaluate(&mut e).unwrap();
        assert_eq!("l", res.result);
    }

    #[test]
    fn test_read_more_than_input() {
        let input = "l";
        let c = EvalConfigBuilder::new(
            Cursor::new(input),
            r#"local m = require('@lam'); return m.read(3)"#,
        )
        .build();
        let mut e = Evaluation::new(c);
        let res = evaluate(&mut e).unwrap();
        assert_eq!("l", res.result);
    }

    #[test]
    fn test_read_unicode() {
        let input = "你好";
        let c = EvalConfigBuilder::new(
            Cursor::new(input),
            r#"local m = require('@lam'); return m.read_unicode(1)"#,
        )
        .build();
        let mut e = Evaluation::new(c);
        let res = evaluate(&mut e).unwrap();
        assert_eq!("你", res.result);
    }

    #[test]
    fn test_read_line() {
        let input = "foo\nbar";
        let c = EvalConfigBuilder::new(
            Cursor::new(input),
            r#"local m = require('@lam'); m.read('*l'); return m.read('*l')"#,
        )
        .build();
        let mut e = Evaluation::new(c);
        let res = evaluate(&mut e).unwrap();
        assert_eq!("bar", res.result);
    }

    #[test]
    fn test_read_number() {
        let input = "3.1415926";
        let c = EvalConfigBuilder::new(
            Cursor::new(input),
            r#"local m = require('@lam'); return m.read('*n')"#,
        )
        .build();
        let mut e = Evaluation::new(c);
        let res = evaluate(&mut e).unwrap();
        assert_eq!("3.1415926", res.result);
    }

    #[test]
    fn test_read_integer() {
        let input = "3";
        let c = EvalConfigBuilder::new(
            Cursor::new(input),
            r#"local m = require('@lam'); return m.read('*n')"#,
        )
        .build();
        let mut e = Evaluation::new(c);
        let res = evaluate(&mut e).unwrap();
        assert_eq!("3", res.result);
    }

    #[test]
    fn test_reevaluate() {
        let input = "foo\nbar";

        let c = EvalConfigBuilder::new(
            Cursor::new(input),
            r#"local m = require('@lam'); return m.read('*l')"#,
        )
        .build();
        let mut e = Evaluation::new(c);

        let res = evaluate(&mut e).unwrap();
        assert_eq!("foo\n", res.result);

        let res = evaluate(&mut e).unwrap();
        assert_eq!("bar", res.result);
    }

    #[test]
    fn test_handle_binary() {
        let input = &[1, 2, 3];
        let c = EvalConfigBuilder::new(
            Cursor::new(input),
            r#"local m = require('@lam'); local a = m.read('*a'); return #a"#,
        )
        .build();
        let mut e = Evaluation::new(c);
        let res = evaluate(&mut e).unwrap();
        assert_eq!("3", res.result);
    }
}
