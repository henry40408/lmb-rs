use bitcode::{Decode, Encode};
use include_dir::{include_dir, Dir};
use parking_lot::Mutex;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead as _, BufReader, Read},
    sync::Arc,
    time::{Duration, Instant},
};

use dashmap::DashMap;
use mlua::LuaSerdeExt as _;
use mlua::{Table, ThreadStatus, UserData, VmState};
use thiserror::Error;
use tracing::{debug, error};

const DEFAULT_TIMEOUT: u64 = 30;
const K_LOADED: &str = "_LOADED";

static MIGRATIONS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/migrations");

pub struct LamStore {
    pub conn: Connection,
}

impl LamStore {
    pub fn migrate(&self) -> LamResult<()> {
        for e in MIGRATIONS_DIR.entries() {
            let sql = e
                .as_file()
                .expect("invalid file")
                .contents_utf8()
                .expect("invalid contents");
            self.conn.execute(sql, ())?;
        }
        Ok(())
    }

    pub fn set<S: AsRef<str>>(&self, name: S, value: &LamValue) -> LamResult<()> {
        let name = name.as_ref();
        let value = bitcode::encode(value);
        self.conn.execute(
            r#"
            INSERT INTO store (name, value) VALUES (?1, ?2)
            ON CONFLICT(name) DO UPDATE SET value = ?2
            "#,
            (name, value),
        )?;
        Ok(())
    }

    pub fn get<S: AsRef<str>>(&self, name: S) -> LamResult<LamValue> {
        let name = name.as_ref();
        let v: Vec<u8> = match self.conn.query_row(
            r#"SELECT value FROM store WHERE name = ?1"#,
            (name,),
            |row| row.get(0),
        ) {
            Err(_) => return Ok(LamValue::None),
            Ok(v) => v,
        };
        Ok(bitcode::decode::<LamValue>(&v)?)
    }
}

#[derive(Debug, Error)]
pub enum LamError {
    #[error("lua error: {0}")]
    Lua(#[from] mlua::Error),
    #[error("bitcode error: {0}")]
    Bitcode(#[from] bitcode::Error),
    #[error("sqlite error: {0}")]
    SQLite(#[from] rusqlite::Error),
}

pub type LamInput<R> = Arc<Mutex<BufReader<R>>>;
pub type LamKV = Arc<DashMap<String, LamValue>>;
pub type LamResult<T> = Result<T, LamError>;

pub struct Evaluation<R>
where
    for<'lua> R: Read + 'lua,
{
    pub input: Arc<Mutex<BufReader<R>>>,
    pub script: String,
    pub store: LamKV,
    pub timeout: u64,
}

pub struct LuaLam<R>
where
    R: Read,
{
    input: LamInput<R>,
    store: LamKV,
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
            let mut input = this.input.lock();
            if let Some(f) = f.as_str() {
                if f == "*a" || f == "*all" {
                    // accepts *a or *all
                    let mut buf = Vec::new();
                    let count = input.read_to_end(&mut buf)?;
                    if count == 0 {
                        return Ok(mlua::Value::Nil);
                    }
                    let s = String::from_utf8(buf).unwrap_or_default();
                    return Ok(mlua::Value::String(vm.create_string(s)?));
                }
                if f == "*l" || f == "*line" {
                    // accepts *l or *line
                    let mut buf = String::new();
                    let count = input.read_line(&mut buf)?;
                    if count == 0 {
                        return Ok(mlua::Value::Nil);
                    }
                    // in Lua, *l doesn't include newline character
                    return Ok(mlua::Value::String(vm.create_string(buf.trim_end())?));
                }
                if f == "*n" || f == "*number" {
                    // accepts *n or *number
                    let mut buf = String::new();
                    let count = input.read_to_string(&mut buf)?;
                    if count == 0 {
                        return Ok(mlua::Value::Nil);
                    }
                    return Ok(buf
                        .parse::<f64>()
                        .map(mlua::Value::Number)
                        .unwrap_or(mlua::Value::Nil));
                }
            }

            if let Some(i) = f.as_usize() {
                let mut buf = vec![0; i];
                let count = input.read(&mut buf)?;
                if count == 0 {
                    return Ok(mlua::Value::Nil);
                }
                buf.truncate(count);
                let s = vm.create_string(buf)?;
                return Ok(mlua::Value::String(s));
            }

            let s = format!("unexpected format {f:?}");
            Err(mlua::Error::RuntimeError(s))
        });

        methods.add_method("read_unicode", |_, this, i: Option<u64>| {
            let mut input = this.input.lock();
            let mut expected_read = i.unwrap_or(1);
            let mut buf = Vec::new();
            let mut byte_buf = vec![0; 1];
            loop {
                if expected_read == 0 {
                    let s = String::from_utf8(buf).unwrap_or_default();
                    return Ok(Some(s));
                }
                let read_bytes = input.read(&mut byte_buf)?;
                if read_bytes == 0 {
                    if buf.is_empty() {
                        return Ok(None);
                    }
                    let s = String::from_utf8(buf).unwrap_or_default();
                    return Ok(Some(s));
                }
                if read_bytes > 0 {
                    buf.extend_from_slice(&byte_buf);
                }
                if std::str::from_utf8(&buf).is_ok() {
                    expected_read -= 1;
                }
            }
        });

        methods.add_method("get", |vm, this, key: String| {
            if let Some(v) = this.store.get(key.as_str()) {
                return vm.to_value(&v.clone());
            }
            Ok(mlua::Value::Nil)
        });

        methods.add_method(
            "set",
            |vm, this, (key, value): (String, mlua::Value<'lua>)| {
                this.store.insert(key, vm.from_value(value.clone())?);
                Ok(value)
            },
        );

        methods.add_method(
            "update",
            |vm, this, (key, f, default_v): (String, mlua::Function<'lua>, mlua::Value<'lua>)| {
                let new_v = this
                    .store
                    .entry(key)
                    .and_modify(|old| match vm.to_value(old) {
                        Ok(old_v) => match f.call(old_v) {
                            Ok(ret) => match vm.from_value(ret) {
                                Ok(new) => {
                                    *old = new;
                                }
                                Err(err) => {
                                    error!(?err, "failed to convert returned value");
                                }
                            },
                            Err(err) => {
                                error!(?err, "failed to run the function");
                            }
                        },
                        Err(err) => {
                            error!(?err, "failed to convert store value");
                        }
                    })
                    .or_insert_with(|| match vm.from_value(default_v) {
                        Ok(v) => v,
                        Err(_) => LamValue::None,
                    })
                    .value()
                    .clone();
                vm.to_value(&new_v)
            },
        );
    }
}

impl<R> LuaLam<R>
where
    R: Read,
{
    pub fn new(input: LamInput<R>, store: LamKV) -> Self {
        Self { input, store }
    }
}

#[derive(Clone, Debug, PartialEq, Encode, Decode, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LamValue {
    None,
    Boolean(bool),
    Number(f64), // represent float and integer
    String(String),
}

impl UserData for LamValue {}

pub struct EvalBuilder<R>
where
    R: Read,
{
    pub input: R,
    pub script: String,
    pub store: LamKV,
    pub timeout: Option<u64>,
}

impl<R> EvalBuilder<R>
where
    for<'lua> R: Read + 'lua,
{
    pub fn new<S: AsRef<str>>(input: R, script: S) -> Self {
        Self {
            input,
            script: script.as_ref().to_string(),
            store: LamKV::default(),
            timeout: None,
        }
    }

    pub fn set_timeout(mut self, timeout: u64) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn set_store(mut self, store: LamKV) -> Self {
        self.store = store;
        self
    }

    pub fn build(self) -> Evaluation<R> {
        Evaluation {
            input: Arc::new(Mutex::new(BufReader::new(self.input))),
            script: self.script,
            store: self.store,
            timeout: self.timeout.unwrap_or(DEFAULT_TIMEOUT),
        }
    }
}

#[derive(Debug)]
pub struct EvalResult {
    pub duration: Duration,
    pub result: String,
}

pub fn evaluate<R>(e: &Evaluation<R>) -> LamResult<EvalResult>
where
    for<'lua> R: Read + 'lua,
{
    let vm = mlua::Lua::new();
    vm.sandbox(true)?;

    let start = Instant::now();

    let timeout = e.timeout as f64;
    let script = &e.script;
    debug!(%timeout, %script, "load script");

    vm.set_interrupt(move |_| {
        if start.elapsed().as_secs_f64() > timeout {
            return Ok(VmState::Yield);
        }
        Ok(VmState::Continue)
    });

    let r = vm.scope(|_| {
        let loaded = vm.named_registry_value::<Table<'_>>(K_LOADED)?;

        let lua_lam = LuaLam::new(e.input.clone(), e.store.clone());
        loaded.set("@lam", lua_lam)?;

        vm.set_named_registry_value(K_LOADED, loaded)?;

        let co = vm.create_thread(vm.load(&e.script).into_function()?)?;
        loop {
            let res = co.resume::<_, Option<String>>(())?;
            if co.status() != ThreadStatus::Resumable
                || start.elapsed().as_secs_f64() > e.timeout as f64
            {
                let duration = start.elapsed();
                let result = res.unwrap_or(String::new());
                debug!(?duration, %result, "evaluation finished");
                return Ok(EvalResult { duration, result });
            }
        }
    })?;
    Ok(r)
}

#[cfg(test)]
mod test {
    use std::{fs, io::Cursor, sync::Arc, thread};

    use dashmap::DashMap;
    use rusqlite::Connection;

    use crate::{evaluate, EvalBuilder, LamStore, LamValue};

    const TIMEOUT_THRESHOLD: f32 = 0.01;

    #[test]
    fn test_evaluate_examples() {
        let cases = [
            ["01-hello.lua", "", ""],
            ["02-input.lua", "lua", ""],
            ["03-algebra.lua", "2", "4"],
            ["04-echo.lua", "a", "a"],
            ["05-state.lua", "", "0"],
        ];
        for case in cases {
            let [filename, input, expected] = case;
            let script = fs::read_to_string(format!("./lua-examples/{filename}")).unwrap();
            let e = EvalBuilder::new(Cursor::new(input), &script).build();
            let res = evaluate(&e).unwrap();
            assert_eq!(
                expected, res.result,
                "expect result of {script} to equal to {expected}"
            );
        }
    }

    #[test]
    fn test_evaluate_infinite_loop() {
        let timeout = 1;

        let input: &[u8] = &[];
        let e = EvalBuilder::new(input, r#"while true do end"#)
            .set_timeout(timeout)
            .build();
        let res = evaluate(&e).unwrap();
        assert_eq!("", res.result);

        let secs = res.duration.as_secs_f32();
        let to = timeout as f32;
        assert!((secs - to) / to < TIMEOUT_THRESHOLD, "timed out {}s", secs);
    }

    #[test]
    fn test_evaluate_scripts() {
        let cases = [
            ["return 1+1", "2"],
            ["return 'a'..1", "a1"],
            ["return require('@lam')._VERSION", "0.1.0"],
        ];
        for case in cases {
            let [script, expected] = case;
            let e = EvalBuilder::new(Cursor::new(""), script).build();
            let res = evaluate(&e).unwrap();
            assert_eq!(
                expected, res.result,
                "expect result of {script} to equal to {expected}"
            );
        }
    }

    #[test]
    fn test_read() {
        let cases = [
            [r#"return require('@lam'):read('*a')"#, "foo\nbar"],
            [r#"return require('@lam'):read('*l')"#, "foo"],
            [r#"return require('@lam'):read(1)"#, "f"],
            [r#"return require('@lam'):read(4)"#, "foo\n"],
        ];
        for case in cases {
            let input = "foo\nbar";
            let [script, expected] = case;
            let e = EvalBuilder::new(Cursor::new(input), script).build();
            let res = evaluate(&e).unwrap();
            assert_eq!(
                expected, res.result,
                "expect result of {script} to equal to {expected}"
            );
        }

        let script = r#"return require('@lam'):read('*n')"#;
        let cases = [
            ["1", "1"],
            ["1.2", "1.2"],
            ["1.23e-10", "1.23e-10"],
            ["3.1415926", "3.1415926"],
            ["", ""],
            ["NaN", "nan"],
            ["InvalidNumber", ""],
        ];
        for case in cases {
            let [input, expected] = case;
            let e = EvalBuilder::new(Cursor::new(input), script).build();
            let res = evaluate(&e).unwrap();
            assert_eq!(
                expected, res.result,
                "expect result of {script} to equal to {expected}"
            );
        }
    }

    #[test]
    fn test_read_binary() {
        let input: &[u8] = &[1, 2, 3];
        let e = EvalBuilder::new(input, r#"return #require('@lam'):read('*a')"#).build();
        let res = evaluate(&e).unwrap();
        assert_eq!("3", res.result);
    }

    #[test]
    fn test_read_empty() {
        let scripts = [
            r#"assert(not require('@lam'):read('*a'))"#,
            r#"assert(not require('@lam'):read('*l'))"#,
            r#"assert(not require('@lam'):read('*n'))"#,
            r#"assert(not require('@lam'):read(1))"#,
        ];
        for script in scripts {
            let input: &[u8] = &[];
            let e = EvalBuilder::new(input, script).build();
            let _ = evaluate(&e).unwrap();
        }
    }

    #[test]
    fn test_read_unicode() {
        let input = "你好";
        let e = EvalBuilder::new(
            Cursor::new(input),
            r#"return require('@lam'):read_unicode(1)"#,
        )
        .build();
        let res = evaluate(&e).unwrap();
        assert_eq!("你", res.result);

        let input = r#"{"key":"你好"}"#;
        let e = EvalBuilder::new(
            Cursor::new(input),
            r#"return require('@lam'):read_unicode(12)"#,
        )
        .build();
        let res = evaluate(&e).unwrap();
        assert_eq!(input, res.result);
    }

    #[test]
    fn test_reevaluate() {
        let input = "foo\nbar";

        let script = r#"return require('@lam'):read('*l')"#;
        let e = EvalBuilder::new(Cursor::new(input), script).build();

        let res = evaluate(&e).unwrap();
        assert_eq!("foo", res.result);

        let res = evaluate(&e).unwrap();
        assert_eq!("bar", res.result);
    }

    #[test]
    fn test_reuse_store() {
        let input: &[u8] = &[];

        let store = Arc::new(DashMap::new());
        store.insert("a".to_string(), LamValue::Number(1f64));

        let e = EvalBuilder::new(
            input,
            r#"local m = require('@lam'); local a = m:get('a'); m:set('a', a+1); return a"#,
        )
        .set_store(store)
        .build();

        {
            let res = evaluate(&e).unwrap();
            assert_eq!("1", res.result);
            assert_eq!(LamValue::Number(2f64), *e.store.get("a").unwrap());
        }

        {
            let res = evaluate(&e).unwrap();
            assert_eq!("2", res.result);
            assert_eq!(LamValue::Number(3f64), *e.store.get("a").unwrap());
        }
    }

    #[test]
    fn test_store() {
        let input: &[u8] = &[];

        let store = Arc::new(DashMap::new());
        store.insert("a".to_string(), LamValue::Number(1.23));

        let e = EvalBuilder::new(
            input,
            r#"local m = require('@lam'); local a = m:get('a'); m:set('a', 4.56); return a"#,
        )
        .set_store(store)
        .build();

        let res = evaluate(&e).unwrap();
        assert_eq!("1.23", res.result);
        assert_eq!(LamValue::Number(4.56), *e.store.get("a").unwrap());
    }

    #[test]
    fn test_rollback_when_update() {
        let input: &[u8] = &[];

        let store = Arc::new(DashMap::new());
        store.insert("a".to_string(), LamValue::Number(1f64));

        let e = EvalBuilder::new(
            input,
            r#"return require('@lam'):update('a', function(v)
              if v == 1 then
                error('something went wrong')
              else
                return v+1
              end
            end, 0)"#,
        )
        .set_store(store)
        .build();

        let res = evaluate(&e).unwrap();
        assert_eq!("1", res.result);
        assert_eq!(LamValue::Number(1f64), *e.store.get("a").unwrap());
    }

    #[test]
    fn test_store_concurrency() {
        let input: &[u8] = &[];

        let store = Arc::new(DashMap::new());

        let mut threads = vec![];
        for _ in 0..=1000 {
            let store = store.clone();
            threads.push(thread::spawn(move || {
                let e = EvalBuilder::new(
                    input,
                    r#"return require('@lam'):update('a', function(v) return v+1 end, 0)"#,
                )
                .set_store(store)
                .build();
                evaluate(&e).unwrap();
            }));
        }
        for t in threads {
            let _ = t.join();
        }
        assert_eq!(LamValue::Number(1000f64), *store.get("a").unwrap());
    }

    #[test]
    fn test_migrate() {
        let conn = Connection::open_in_memory().unwrap();
        let store = LamStore { conn };
        store.migrate().unwrap();
        store.migrate().unwrap(); // duplicated
    }

    #[test]
    fn test_store_set_get() {
        let conn = Connection::open_in_memory().unwrap();
        let store = LamStore { conn };
        store.migrate().unwrap();

        assert_eq!(store.get("x").unwrap(), LamValue::None);

        let ni = LamValue::None;
        store.set("nil", &ni).unwrap();
        assert_eq!(store.get("nil").unwrap(), ni);

        let b = LamValue::Boolean(true);
        store.set("b", &b).unwrap();
        assert_eq!(store.get("b").unwrap(), b);

        store.set("b", &LamValue::Boolean(false)).unwrap();
        assert_eq!(store.get("b").unwrap(), LamValue::Boolean(false));

        let n = LamValue::Number(1f64);
        store.set("n", &n).unwrap();
        assert_eq!(store.get("n").unwrap(), n);

        store.set("n", &LamValue::Number(2f64)).unwrap();
        assert_eq!(store.get("n").unwrap(), LamValue::Number(2f64));

        let s = LamValue::String("hello".to_string());
        store.set("s", &s).unwrap();
        assert_eq!(store.get("s").unwrap(), s);

        store
            .set("s", &LamValue::String("world".to_string()))
            .unwrap();
        assert_eq!(
            store.get("s").unwrap(),
            LamValue::String("world".to_string())
        );
    }
}
