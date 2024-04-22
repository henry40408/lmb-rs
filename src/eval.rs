use crate::*;
use mlua::prelude::*;
use std::{
    io::{BufReader, Read},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tracing::{debug, trace_span};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

pub struct EvalBuilder<R>
where
    R: Read,
{
    pub input: Option<R>,
    pub name: Option<String>,
    pub script: String,
    pub store: Option<LamStore>,
    pub timeout: Option<Duration>,
}

impl<R> EvalBuilder<R>
where
    for<'lua> R: Read + 'lua,
{
    pub fn set_input<S: Read>(self, input: Option<S>) -> EvalBuilder<S> {
        EvalBuilder {
            input,
            name: self.name,
            script: self.script,
            store: self.store,
            timeout: self.timeout,
        }
    }

    pub fn set_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    pub fn set_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn set_store(mut self, store: LamStore) -> Self {
        self.store = Some(store);
        self
    }

    pub fn build(self) -> Evaluation {
        let vm = Lua::new();
        vm.sandbox(true).expect("failed to enable sandbox");

        let input = self.input.map(BufReader::new);
        LuaLam::register(&vm, input, self.store.clone()).expect("failed to register");

        Evaluation {
            name: self.name.unwrap_or_default(),
            script: self.script,
            store: self.store,
            timeout: self.timeout.unwrap_or(DEFAULT_TIMEOUT),
            vm,
        }
    }
}

pub struct NoInput {}

impl Read for NoInput {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        unreachable!()
    }
}

impl EvalBuilder<NoInput> {
    pub fn new<S: AsRef<str>>(script: S) -> Self {
        Self {
            input: None,
            name: None,
            script: script.as_ref().to_string(),
            store: None,
            timeout: None,
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
    pub name: String,
    pub script: String,
    pub store: Option<LamStore>,
    pub timeout: Duration,
    pub vm: Lua,
}

impl Evaluation {
    pub fn evaluate(&self) -> LamResult<EvalResult> {
        let vm = &self.vm;
        let timeout = self.timeout;

        let max_memory = Arc::new(AtomicUsize::new(0));

        let start = Instant::now();
        self.vm.set_interrupt({
            let max_memory = Arc::clone(&max_memory);
            move |vm| {
                let used_memory = vm.used_memory();
                max_memory.fetch_max(used_memory, Ordering::SeqCst);
                Ok(if start.elapsed() > timeout {
                    LuaVmState::Yield
                } else {
                    LuaVmState::Continue
                })
            }
        });

        let chunk = vm.load(&self.script).set_name(&self.name);
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
    use std::{fs, time::Duration};
    use test_case::test_case;

    #[test_case("./lua-examples/07-error.lua")]
    fn error_in_script(path: &str) {
        let store = LamStore::default();
        let script = fs::read_to_string(path).unwrap();
        let e = EvalBuilder::new(script).set_store(store).build();
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
        let store = LamStore::default();
        let script = fs::read_to_string(format!("./lua-examples/{filename}")).unwrap();
        let e = EvalBuilder::new(&script)
            .set_input(Some(input.as_bytes()))
            .set_store(store)
            .build();
        let res = e.evaluate().expect(&script);
        assert_eq!(expected, res.result);
    }

    #[test]
    fn evaluate_infinite_loop() {
        let timeout = Duration::from_secs(1);

        let e = EvalBuilder::new(r#"while true do end"#)
            .set_timeout(Some(timeout))
            .build();
        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::None, res.result);

        let duration = res.duration;
        assert_eq!(timeout.as_secs(), duration.as_secs());
    }

    #[test_case("return 1+1", "2")]
    #[test_case("return 'a'..1", "a1")]
    #[test_case("return require('@lam')._VERSION", "0.1.0")]
    fn evaluate_scripts(script: &str, expected: &str) {
        let e = EvalBuilder::new(script).build();
        let res = e.evaluate().expect(script);
        assert_eq!(expected, res.result.to_string());
    }

    #[test]
    fn reevaluate() {
        let input = "foo\nbar";
        let script = r#"return require('@lam'):read('*l')"#;
        let e = EvalBuilder::new(script)
            .set_input(Some(input.as_bytes()))
            .build();

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
        let e = EvalBuilder::new(script).build();
        let res = e.evaluate().expect(script);
        assert_eq!(expected, res.result.to_string());
    }

    #[test]
    fn syntax_error() {
        let script = "ret true"; // code with syntax error
        let e = EvalBuilder::new(script).build();
        assert!(e.evaluate().is_err());
    }
}
