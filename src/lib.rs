use std::{
    io::{BufRead as _, BufReader, Read},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use dashmap::DashMap;
use mlua::{FromLua, IntoLua, Lua, Table, ThreadStatus, UserData, VmState};
use thiserror::Error;
use tracing::error;

const DEFAULT_TIMEOUT: u64 = 30;
const K_LOADED: &str = "_LOADED";

#[derive(Debug, Error)]
pub enum LamError {
    #[error("lua error: {0}")]
    Lua(#[from] mlua::Error),
}

pub type LamInput<R> = Arc<Mutex<BufReader<R>>>;
pub type LamResult<T> = Result<T, LamError>;
pub type LamState = Arc<DashMap<String, LamValue>>;

pub struct Evaluation<R>
where
    R: Read,
{
    pub vm: mlua::Lua,
    pub input: Arc<Mutex<BufReader<R>>>,
    pub script: String,
    pub state: Arc<DashMap<String, LamValue>>,
    pub timeout: u64,
}

pub struct LuaLam<R>
where
    R: Read,
{
    input: LamInput<R>,
    state: LamState,
}

impl<R> UserData for LuaLam<R>
where
    R: Read,
{
    fn add_fields<'lua, F: mlua::prelude::LuaUserDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field("_VERSION", env!("CARGO_PKG_VERSION"));
    }

    fn add_methods<'lua, M: mlua::prelude::LuaUserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("read", |vm, this, f: mlua::Value<'lua>| {
            let mut input = this.input.lock().expect("failed to lock input for read");
            if let Some(f) = f.as_str() {
                if f.starts_with("*a") {
                    // accepts *a or *all
                    let mut buf = Vec::new();
                    input.read_to_end(&mut buf)?;
                    let s = vm.create_string(String::from_utf8(buf).unwrap_or_default())?;
                    return Ok(mlua::Value::String(s));
                }
                if f.starts_with("*l") {
                    // accepts *l or *line
                    let mut buf = String::new();
                    input.read_line(&mut buf)?;
                    let s = vm.create_string(buf)?;
                    return Ok(mlua::Value::String(s));
                }
                if f.starts_with("*n") {
                    // accepts *n or *number
                    let mut buf = String::new();
                    input.read_to_string(&mut buf)?;
                    return Ok(buf
                        .parse::<f64>()
                        .map(mlua::Value::Number)
                        .unwrap_or(mlua::Value::Nil));
                }
            }

            #[allow(clippy::unused_io_amount)]
            if let Some(i) = f.as_usize() {
                let mut buf = vec![0; i];
                let count = input.read(&mut buf)?;
                buf.truncate(count);
                let s = vm.create_string(String::from_utf8(buf).unwrap_or_default())?;
                return Ok(mlua::Value::String(s));
            }

            let s = format!("unexpected format {f:?}");
            Err(mlua::Error::RuntimeError(s))
        });

        methods.add_method("read_unicode", |_vm, this, i: u64| {
            let mut input = this
                .input
                .lock()
                .expect("failed to lock input for read_unicode");
            let mut expected_read = i;
            let mut buf = Vec::new();
            let mut byte_buf = vec![0; 1];
            loop {
                if expected_read == 0 {
                    return Ok(String::from_utf8(buf).unwrap_or_default());
                }
                let read_bytes = input.read(&mut byte_buf)?;
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
        });

        methods.add_method("get", |vm, this, key: String| {
            if let Some(v) = this.state.get(key.as_str()) {
                return v.clone().into_lua(vm);
            }
            Ok(mlua::Value::Nil)
        });

        methods.add_method(
            "set",
            |vm, this, (key, value): (String, mlua::Value<'lua>)| {
                this.state.insert(key, LamValue::from_lua(value, vm)?);
                Ok(())
            },
        );

        methods.add_method(
            "get_set",
            |vm, this, (key, f, default_v): (String, mlua::Function<'lua>, mlua::Value<'lua>)| {
                Ok(this
                    .state
                    .entry(key)
                    .and_modify(|v| match f.call(v.clone().into_lua(vm)) {
                        Ok(ret_v) => match LamValue::from_lua(ret_v, vm) {
                            Ok(state_v) => {
                                *v = state_v;
                            }
                            Err(e) => {
                                error!("failed to convert lua value {:?}", e);
                            }
                        },
                        Err(e) => {
                            error!("failed to run lua function {:?}", e);
                        }
                    })
                    .or_insert(LamValue::from_lua(default_v, vm)?)
                    .value()
                    .clone())
            },
        );
    }
}

impl<R> LuaLam<R>
where
    R: Read,
{
    pub fn new(input: LamInput<R>, state: LamState) -> Self {
        Self { input, state }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum LamValue {
    None,
    Boolean(bool),
    Number(f64), // represent float and integer
    String(String),
}

impl<'lua> IntoLua<'lua> for LamValue {
    fn into_lua(self, lua: &'lua Lua) -> mlua::prelude::LuaResult<mlua::prelude::LuaValue<'lua>> {
        match self {
            Self::None => Ok(mlua::Value::Nil),
            Self::Boolean(b) => b.into_lua(lua),
            Self::Number(n) => n.into_lua(lua),
            Self::String(s) => s.into_lua(lua),
        }
    }
}

impl<'lua> FromLua<'lua> for LamValue {
    fn from_lua(
        value: mlua::prelude::LuaValue<'lua>,
        _lua: &'lua Lua,
    ) -> mlua::prelude::LuaResult<Self> {
        if let Some(b) = value.as_boolean() {
            return Ok(Self::Boolean(b));
        }
        if let Some(n) = value.as_i64() {
            return Ok(Self::Number(n as f64));
        }
        if let Some(n) = value.as_f64() {
            return Ok(Self::Number(n));
        }
        if let Some(s) = value.as_str() {
            return Ok(Self::String(s.to_string()));
        }
        Ok(Self::None)
    }
}

pub struct EvalBuilder<R>
where
    R: Read,
{
    pub input: R,
    pub script: String,
    pub state: Option<LamState>,
    pub timeout: Option<u64>,
}

impl<R> EvalBuilder<R>
where
    R: Read,
{
    pub fn new<S: AsRef<str>>(input: R, script: S) -> Self {
        Self {
            input,
            script: script.as_ref().to_string(),
            state: None,
            timeout: None,
        }
    }

    pub fn set_timeout(mut self, timeout: u64) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn set_state(mut self, state: LamState) -> Self {
        self.state = Some(state);
        self
    }

    pub fn build(self) -> LamResult<Evaluation<R>> {
        let vm = mlua::Lua::new();
        vm.sandbox(true)?;
        Ok(Evaluation {
            vm,
            input: Arc::new(Mutex::new(BufReader::new(self.input))),
            script: self.script,
            state: self.state.unwrap_or_default(),
            timeout: self.timeout.unwrap_or(DEFAULT_TIMEOUT),
        })
    }
}

#[derive(Debug)]
pub struct EvalResult {
    pub duration: Duration,
    pub result: String,
}

pub fn evaluate<R>(e: &Evaluation<R>) -> LamResult<EvalResult>
where
    R: Read + 'static,
{
    let vm = &e.vm;

    let start = Instant::now();
    let timeout = e.timeout as f64;
    vm.set_interrupt(move |_| {
        if start.elapsed().as_secs_f64() > timeout {
            return Ok(VmState::Yield);
        }
        Ok(VmState::Continue)
    });

    let r = vm.scope(|_| {
        let loaded = vm.named_registry_value::<Table<'_>>(K_LOADED)?;

        let lua_lam = LuaLam::new(e.input.clone(), e.state.clone());
        loaded.set("@lam", lua_lam)?;

        vm.set_named_registry_value(K_LOADED, loaded)?;

        let co = vm.create_thread(vm.load(&e.script).into_function()?)?;
        loop {
            let res = co.resume::<_, Option<String>>(())?;
            if co.status() != ThreadStatus::Resumable
                || start.elapsed().as_secs_f64() > e.timeout as f64
            {
                return Ok(EvalResult {
                    duration: start.elapsed(),
                    result: res.unwrap_or(String::new()),
                });
            }
        }
    })?;
    Ok(r)
}

#[cfg(test)]
mod test {
    use std::{io::Cursor, sync::Arc, thread};

    use dashmap::DashMap;

    use crate::{evaluate, EvalBuilder, LamValue};

    const TIMEOUT_THRESHOLD: f32 = 0.01;

    #[test]
    fn test_evaluate_infinite_loop() {
        let timeout = 1;

        let input: &[u8] = &[];
        let e = EvalBuilder::new(input, r#"while true do end"#)
            .set_timeout(timeout)
            .build()
            .unwrap();
        let res = evaluate(&e).unwrap();
        assert_eq!("", res.result);

        let secs = res.duration.as_secs_f32();
        let to = timeout as f32;
        assert!((secs - to) / to < TIMEOUT_THRESHOLD, "timed out {}s", secs);
    }

    #[test]
    fn test_read_all() {
        let input = "lam";
        let e = EvalBuilder::new(
            Cursor::new(input),
            r#"local m = require('@lam'); return m:read('*a')"#,
        )
        .build()
        .unwrap();
        let res = evaluate(&e).unwrap();
        assert_eq!(input, res.result);
    }

    #[test]
    fn test_read_partial_input() {
        let input = "lam";
        let e = EvalBuilder::new(
            Cursor::new(input),
            r#"local m = require('@lam'); return m:read(1)"#,
        )
        .build()
        .unwrap();
        let res = evaluate(&e).unwrap();
        assert_eq!("l", res.result);
    }

    #[test]
    fn test_read_more_than_input() {
        let input = "l";
        let e = EvalBuilder::new(
            Cursor::new(input),
            r#"local m = require('@lam'); return m:read(3)"#,
        )
        .build()
        .unwrap();
        let res = evaluate(&e).unwrap();
        assert_eq!("l", res.result);
    }

    #[test]
    fn test_read_unicode() {
        let input = "你好";
        let e = EvalBuilder::new(
            Cursor::new(input),
            r#"local m = require('@lam'); return m:read_unicode(1)"#,
        )
        .build()
        .unwrap();
        let res = evaluate(&e).unwrap();
        assert_eq!("你", res.result);
    }

    #[test]
    fn test_read_line() {
        let input = "foo\nbar";
        let e = EvalBuilder::new(
            Cursor::new(input),
            r#"local m = require('@lam'); m:read('*l'); return m:read('*l')"#,
        )
        .build()
        .unwrap();
        let res = evaluate(&e).unwrap();
        assert_eq!("bar", res.result);
    }

    #[test]
    fn test_read_number() {
        let input = "3.1415926";
        let e = EvalBuilder::new(
            Cursor::new(input),
            r#"local m = require('@lam'); return m:read('*n')"#,
        )
        .build()
        .unwrap();
        let res = evaluate(&e).unwrap();
        assert_eq!("3.1415926", res.result);
    }

    #[test]
    fn test_read_integer() {
        let input = "3";
        let e = EvalBuilder::new(
            Cursor::new(input),
            r#"local m = require('@lam'); return m:read('*n')"#,
        )
        .build()
        .unwrap();
        let res = evaluate(&e).unwrap();
        assert_eq!("3", res.result);
    }

    #[test]
    fn test_reevaluate() {
        let input = "foo\nbar";

        let e = EvalBuilder::new(
            Cursor::new(input),
            r#"local m = require('@lam'); return m:read('*l')"#,
        )
        .build()
        .unwrap();

        let res = evaluate(&e).unwrap();
        assert_eq!("foo\n", res.result);

        let res = evaluate(&e).unwrap();
        assert_eq!("bar", res.result);
    }

    #[test]
    fn test_handle_binary() {
        let input: &[u8] = &[1, 2, 3];
        let e = EvalBuilder::new(
            input,
            r#"local m = require('@lam'); local a = m:read('*a'); return #a"#,
        )
        .build()
        .unwrap();
        let res = evaluate(&e).unwrap();
        assert_eq!("3", res.result);
    }

    #[test]
    fn test_state() {
        let input: &[u8] = &[];

        let state = DashMap::new();
        state.insert("a".to_string(), LamValue::Number(1.23));

        let e = EvalBuilder::new(
            input,
            r#"local m = require('@lam'); local a = m:get('a'); m:set('a', 4.56); return a"#,
        )
        .set_state(Arc::new(state))
        .build()
        .unwrap();

        let res = evaluate(&e).unwrap();
        assert_eq!("1.23", res.result);
        assert_eq!(LamValue::Number(4.56), *e.state.get("a").unwrap());
    }

    #[test]
    fn test_reuse_state() {
        let input: &[u8] = &[];

        let state = DashMap::new();
        state.insert("a".to_string(), LamValue::Number(1f64));

        let e = EvalBuilder::new(
            input,
            r#"local m = require('@lam'); local a = m:get('a'); m:set('a', a+1); return a"#,
        )
        .set_state(Arc::new(state))
        .build()
        .unwrap();

        {
            let res = evaluate(&e).unwrap();
            assert_eq!("1", res.result);
            assert_eq!(LamValue::Number(2f64), *e.state.get("a").unwrap());
        }

        {
            let res = evaluate(&e).unwrap();
            assert_eq!("2", res.result);
            assert_eq!(LamValue::Number(3f64), *e.state.get("a").unwrap());
        }
    }

    #[test]
    fn test_state_concurrency() {
        let input: &[u8] = &[];

        let state = Arc::new(DashMap::new());

        let mut threads = vec![];
        for _ in 0..=1000 {
            let state = state.clone();
            threads.push(thread::spawn(move || {
                let e = EvalBuilder::new(
                    input,
                    r#"local m = require('@lam'); m:get_set('a', function(v) return v+1 end, 0)"#,
                )
                .set_state(state)
                .build()
                .unwrap();
                let _ = evaluate(&e);
            }));
        }
        for t in threads {
            let _ = t.join();
        }
        assert_eq!(LamValue::Number(1000f64), *state.get("a").unwrap());
    }
}
