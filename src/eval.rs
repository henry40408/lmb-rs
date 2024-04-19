use crate::*;
use mlua::prelude::*;
use parking_lot::Mutex;
use std::{
    io::{BufReader, Read},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tracing::{debug, trace_span};

const DEFAULT_TIMEOUT: u64 = 30;

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

    pub fn build(self) -> Evaluation {
        let vm = Lua::new();
        vm.sandbox(true).expect("failed to enable sandbox");

        let compiler = mlua::Compiler::new();
        let compiled = {
            let name = &self.name;
            let script = &self.script;
            let _ = trace_span!("compile script", name, script).entered();
            compiler.compile(&self.script)
        };

        let input = Arc::new(Mutex::new(BufReader::new(self.input)));
        LuaLam::register(&vm, input, self.store.clone()).expect("failed to register");

        Evaluation {
            compiled,
            name: self.name.unwrap_or_default(),
            script: self.script,
            store: self.store,
            timeout: Duration::from_secs(self.timeout.unwrap_or(DEFAULT_TIMEOUT)),
            vm,
        }
    }
}

#[derive(Debug)]
pub struct EvalResult {
    pub duration: Duration,
    pub max_memory: usize,
    pub result: LamValue,
}

pub struct Evaluation {
    pub compiled: Vec<u8>,
    pub name: String,
    pub script: String,
    pub store: LamStore,
    pub timeout: Duration,
    pub vm: Lua,
}

impl Evaluation {
    pub fn evaluate(&self) -> LamResult<EvalResult> {
        let vm = &self.vm;
        let timeout = self.timeout;

        let max_memory = Arc::new(AtomicUsize::new(0));

        let mm_clone = max_memory.clone();
        let start = Instant::now();
        self.vm.set_interrupt(move |vm| {
            let used_memory = vm.used_memory();
            mm_clone.fetch_max(used_memory, Ordering::SeqCst);
            Ok(if start.elapsed() > timeout {
                LuaVmState::Yield
            } else {
                LuaVmState::Continue
            })
        });

        let chunk = vm.load(&self.compiled).set_name(&self.name);
        let co = vm.create_thread(chunk.into_function()?)?;
        let _ = trace_span!("evaluate", name = &self.name).entered();
        loop {
            let result_value = co.resume::<_, LuaValue<'_>>(())?;
            let unresumable = co.status() != LuaThreadStatus::Resumable;
            let duration = start.elapsed();
            let timed_out = duration > self.timeout;
            if unresumable || timed_out {
                let max_memory = max_memory.load(Ordering::SeqCst);
                debug!(?duration, name = &self.name, ?max_memory, "evaluated");
                return Ok(EvalResult {
                    duration,
                    max_memory,
                    result: vm.from_value::<LamValue>(result_value)?,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::*;
    use maplit::hashmap;
    use std::fs;
    use test_case::test_case;

    fn new_store() -> LamStore {
        let store = LamStore::default();
        store.migrate().unwrap();
        store
    }

    #[test_case("./lua-examples/07-error.lua")]
    fn error_in_script(path: &str) {
        let store = new_store();
        let script = fs::read_to_string(path).unwrap();
        let e = EvalBuilder::new(&b""[..], &script).set_store(store).build();
        assert!(e.evaluate().is_err());
    }

    #[test_case("01-hello.lua", "", LamValue::None)]
    #[test_case("02-input.lua", "lua", LamValue::None)]
    #[test_case("03-algebra.lua", "2", 4.into())]
    #[test_case("04-echo.lua", "a", "a".into())]
    #[test_case("05-state.lua", "", 1.into())]
    #[test_case("06-count-bytes.lua", "A", hashmap!{ "65".into() => 1.into() }.into())]
    #[test_case("08-return-table.lua", "123", hashmap!{
        "a".into() => true.into(),
        "b".into() => 1.23.into(),
        "c".into() => "hello".into()
    }.into())]
    #[test_case("09-read-unicode.lua", "你好，世界", "你好".into())]
    fn evaluate_examples(filename: &str, input: &'static str, expected: LamValue) {
        let store = new_store();
        let script = fs::read_to_string(format!("./lua-examples/{filename}")).unwrap();
        let e = EvalBuilder::new(input.as_bytes(), &script)
            .set_store(store)
            .build();
        let res = e.evaluate().expect(&script);
        assert_eq!(expected, res.result);
    }

    #[test]
    fn evaluate_infinite_loop() {
        let timeout = 1;

        let input: &[u8] = &[];
        let e = EvalBuilder::new(input, r#"while true do end"#)
            .set_timeout(timeout)
            .build();
        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::None, res.result);

        let duration = res.duration;
        assert_eq!(timeout, duration.as_secs());
    }

    #[test_case("return 1+1", "2")]
    #[test_case("return 'a'..1", "a1")]
    #[test_case("return require('@lam')._VERSION", "0.1.0")]
    fn evaluate_scripts(script: &str, expected: &str) {
        let e = EvalBuilder::new(&b""[..], script).build();
        let res = e.evaluate().expect(script);
        assert_eq!(expected, res.result.to_string());
    }

    #[test]
    fn reevaluate() {
        let input = "foo\nbar";

        let script = r#"return require('@lam'):read('*l')"#;
        let e = EvalBuilder::new(input.as_bytes(), script).build();

        let res = e.evaluate().unwrap();
        assert_eq!("foo", res.result.to_string());

        let res = e.evaluate().unwrap();
        assert_eq!("bar", res.result.to_string());
    }

    #[test_case(r#""#, "")]
    #[test_case(r#"return nil"#, "")]
    #[test_case(r#"return true"#, "true")]
    #[test_case(r#"return false"#, "false")]
    #[test_case(r#"return 1"#, "1")]
    #[test_case(r#"return 1.23"#, "1.23")]
    #[test_case(r#"return 'hello'"#, "hello")]
    #[test_case(r#"return {a=true,b=1.23,c="hello"}"#, "table: 0x0")]
    #[test_case(r#"return {true,1.23,"hello"}"#, "table: 0x0")]
    fn return_to_string(script: &str, expected: &str) {
        let input: &[u8] = &[];
        let e = EvalBuilder::new(input, script).build();
        let res = e.evaluate().expect(script);
        assert_eq!(expected, res.result.to_string());
    }

    #[test]
    fn syntax_error() {
        let input: &[u8] = &[];
        let script = "ret true"; // code with syntax error
        let e = EvalBuilder::new(input, script).build();
        assert!(e.evaluate().is_err());
    }
}
