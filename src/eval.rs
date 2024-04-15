use crate::*;
use mlua::{LuaSerdeExt as _, Table, ThreadStatus, VmState};
use parking_lot::Mutex;
use std::{
    io::{BufReader, Read},
    sync::Arc,
    time::{Duration, Instant},
};
use tracing::debug;

const DEFAULT_TIMEOUT: u64 = 30;
const K_LOADED: &str = "_LOADED";

pub struct Evaluation<R>
where
    for<'lua> R: Read + 'lua,
{
    pub name: String,
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
    pub name: Option<String>,
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
            name: None,
            script: script.as_ref().to_string(),
            store: LamStore::default(),
            timeout: None,
        }
    }

    pub fn set_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
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
            name: self.name.unwrap_or_default(),
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

pub fn lam_evaluate<R>(e: &Evaluation<R>) -> LamResult<EvalResult>
where
    for<'lua> R: Read + 'lua,
{
    let vm = mlua::Lua::new();
    vm.sandbox(true)?;

    let start = Instant::now();

    let name = &e.name;
    let timeout = e.timeout as f64;
    let script = &e.script;
    debug!(%timeout, ?name,?script, "load script");

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

        let chunk = vm.load(&e.script).set_name(name);
        let co = vm.create_thread(chunk.into_function()?)?;
        loop {
            let result = co.resume::<_, mlua::Value<'_>>(())?;
            let unresumable = co.status() != ThreadStatus::Resumable;
            let timed_out = start.elapsed().as_secs_f64() > e.timeout as f64;
            if unresumable || timed_out {
                let duration = start.elapsed();
                let result = vm.from_value::<LamValue>(result)?;
                let used_memory = vm.used_memory();
                debug!(?duration, ?result, used_memory, "evaluation finished");
                return Ok(EvalResult { duration, result });
            }
        }
    })?;

    Ok(r)
}

#[cfg(test)]
mod tests {
    use crate::*;
    use std::{fs, io::Cursor};

    const TIMEOUT_THRESHOLD: f32 = 0.01;

    fn new_store() -> LamStore {
        let store = LamStore::default();
        store.migrate().unwrap();
        store
    }

    #[test]
    fn error_in_script() {
        let store = new_store();
        let script = fs::read_to_string("./lua-examples/07-error.lua").unwrap();
        let e = EvalBuilder::new(Cursor::new(""), &script)
            .set_store(store)
            .build();
        assert!(lam_evaluate(&e).is_err());
    }

    #[test]
    fn evaluate_examples() {
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
            let res = lam_evaluate(&e).expect(&script);
            assert_eq!(
                expected,
                res.result.to_string(),
                "expect result of {script} to equal to {expected}"
            );
        }
    }

    #[test]
    fn evaluate_infinite_loop() {
        let timeout = 1;

        let input: &[u8] = &[];
        let e = EvalBuilder::new(input, r#"while true do end"#)
            .set_timeout(timeout)
            .build();
        let res = lam_evaluate(&e).unwrap();
        assert_eq!("", res.result.to_string());

        let secs = res.duration.as_secs_f32();
        let to = timeout as f32;
        assert!((secs - to) / to < TIMEOUT_THRESHOLD, "timed out {}s", secs);
    }

    #[test]
    fn evaluate_scripts() {
        let cases = [
            ["return 1+1", "2"],
            ["return 'a'..1", "a1"],
            ["return require('@lam')._VERSION", "0.1.0"],
        ];
        for case in cases {
            let [script, expected] = case;
            let e = EvalBuilder::new(Cursor::new(""), script).build();
            let res = lam_evaluate(&e).expect(script);
            assert_eq!(
                expected,
                res.result.to_string(),
                "expect result of {script} to equal to {expected}"
            );
        }
    }

    #[test]
    fn reevaluate() {
        let input = "foo\nbar";

        let script = r#"return require('@lam'):read('*l')"#;
        let e = EvalBuilder::new(Cursor::new(input), script).build();

        let res = lam_evaluate(&e).unwrap();
        assert_eq!("foo", res.result.to_string());

        let res = lam_evaluate(&e).unwrap();
        assert_eq!("bar", res.result.to_string());
    }

    #[test]
    fn return_to_string() {
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
            let res = lam_evaluate(&e).expect(script);
            assert_eq!(expected, res.result.to_string());
        }
    }
}
