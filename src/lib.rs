use lua_lam::LuaLam;
use mlua::LuaSerdeExt as _;
use mlua::{Table, ThreadStatus, VmState};
use parking_lot::Mutex;
use std::{
    io::{BufReader, Read},
    sync::Arc,
    time::{Duration, Instant},
};
pub use store::LamStore;
use tracing::debug;
pub use value::LamValue;

mod error;
mod lua_lam;
mod store;
mod value;

const DEFAULT_TIMEOUT: u64 = 30;
const K_LOADED: &str = "_LOADED";

pub type LamInput<R> = Arc<Mutex<BufReader<R>>>;
pub type LamResult<T> = Result<T, error::LamError>;

pub struct Evaluation<R>
where
    for<'lua> R: Read + 'lua,
{
    pub input: Arc<Mutex<BufReader<R>>>,
    pub script: String,
    pub store: LamStore,
    pub timeout: u64,
}

pub struct EvalBuilder<R>
where
    R: Read,
{
    pub input: R,
    pub script: String,
    pub store: LamStore,
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
            store: LamStore::default(),
            timeout: None,
        }
    }

    pub fn set_timeout(mut self, timeout: u64) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn set_store(mut self, store: LamStore) -> Self {
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
    pub result: LamValue,
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
    debug!(%timeout, ?script, "load script");

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
            let result = co.resume::<_, mlua::Value<'_>>(())?;
            if co.status() != ThreadStatus::Resumable
                || start.elapsed().as_secs_f64() > e.timeout as f64
            {
                let duration = start.elapsed();
                let result = vm.from_value::<LamValue>(result)?;
                debug!(?duration, ?result, "evaluation finished");
                return Ok(EvalResult { duration, result });
            }
        }
    })?;
    Ok(r)
}

#[cfg(test)]
mod test {
    use std::{fs, io::Cursor};

    use crate::{evaluate, EvalBuilder, LamStore};

    const TIMEOUT_THRESHOLD: f32 = 0.01;

    fn new_store() -> LamStore {
        let store = LamStore::default();
        store.migrate().unwrap();
        store
    }

    #[test]
    fn test_error_in_script() {
        let store = new_store();
        let script = fs::read_to_string("./lua-examples/07-error.lua").unwrap();
        let e = EvalBuilder::new(Cursor::new(""), &script)
            .set_store(store)
            .build();
        assert!(evaluate(&e).is_err());
    }

    #[test]
    fn test_evaluate_examples() {
        let cases = [
            ["01-hello.lua", "", ""],
            ["02-input.lua", "lua", ""],
            ["03-algebra.lua", "2", "4"],
            ["04-echo.lua", "a", "a"],
            ["05-state.lua", "", "1"],
        ];
        for case in cases {
            let store = new_store();
            let [filename, input, expected] = case;
            let script = fs::read_to_string(format!("./lua-examples/{filename}")).unwrap();
            let e = EvalBuilder::new(Cursor::new(input), &script)
                .set_store(store)
                .build();
            let res = evaluate(&e).expect(&script);
            assert_eq!(
                expected,
                res.result.to_string(),
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
        assert_eq!("", res.result.to_string());

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
            let res = evaluate(&e).expect(script);
            assert_eq!(
                expected,
                res.result.to_string(),
                "expect result of {script} to equal to {expected}"
            );
        }
    }

    #[test]
    fn test_reevaluate() {
        let input = "foo\nbar";

        let script = r#"return require('@lam'):read('*l')"#;
        let e = EvalBuilder::new(Cursor::new(input), script).build();

        let res = evaluate(&e).unwrap();
        assert_eq!("foo", res.result.to_string());

        let res = evaluate(&e).unwrap();
        assert_eq!("bar", res.result.to_string());
    }

    #[test]
    fn test_return() {
        let scripts = [
            [r#""#, ""],
            [r#"return nil"#, ""],
            [r#"return true"#, "true"],
            [r#"return false"#, "false"],
            [r#"return 1"#, "1"],
            [r#"return 1.23"#, "1.23"],
            [r#"return 'hello'"#, "hello"],
            [r#"return {a=true,b=1.23,c="hello"}"#, "table: 0x0"],
            [r#"return {true,1.23,"hello"}"#, "table: 0x0"],
        ];
        for [script, expected] in scripts {
            let input: &[u8] = &[];
            let e = EvalBuilder::new(input, script).build();
            let res = evaluate(&e).expect(script);
            assert_eq!(expected, res.result.to_string());
        }
    }
}
