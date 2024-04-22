use crate::*;
use mlua::prelude::*;
use std::{
    borrow::Cow,
    io::{BufReader, Read},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tracing::{debug, trace_span};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

pub struct EvalBuilder<'a, R>
where
    R: Read,
{
    pub input: Option<R>,
    pub name: Option<Cow<'a, str>>,
    pub script: Cow<'a, str>,
    pub store: Option<LamStore>,
    pub timeout: Option<Duration>,
}

impl<'a, R> EvalBuilder<'a, R>
where
    for<'lua> R: Read + 'lua,
{
    pub fn set_input<S: Read>(self, input: Option<S>) -> EvalBuilder<'a, S> {
        EvalBuilder {
            input,
            name: self.name,
            script: self.script,
            store: self.store,
            timeout: self.timeout,
        }
    }

    pub fn set_name(mut self, name: Cow<'a, str>) -> Self {
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

    pub fn build(self) -> Evaluation<'a> {
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

impl<'a> EvalBuilder<'a, NoInput> {
    pub fn new(script: Cow<'a, str>) -> Self {
        Self {
            input: None,
            name: None,
            script,
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

pub struct Evaluation<'a> {
    pub name: Cow<'a, str>,
    pub script: Cow<'a, str>,
    pub store: Option<LamStore>,
    pub timeout: Duration,
    pub vm: Lua,
}

impl<'a> Evaluation<'a> {
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

        let chunk = vm.load(self.script.as_ref()).set_name(self.name.as_ref());
        let co = vm.create_thread(chunk.into_function()?)?;
        let name = self.name.as_ref();
        let _ = trace_span!("evaluate", name).entered();
        loop {
            let result_value = co.resume::<_, LuaValue<'_>>(())?;
            let unresumable = co.status() != LuaThreadStatus::Resumable;
            let duration = start.elapsed();
            let timed_out = duration > self.timeout;
            if unresumable || timed_out {
                let max_memory = max_memory.load(Ordering::SeqCst);
                let name = self.name.as_ref();
                debug!(?duration, name, ?max_memory, "evaluated");
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
    use std::{borrow::Cow, fs, time::Duration};
    use test_case::test_case;

    #[test_case("./lua-examples/07-error.lua")]
    fn error_in_script(path: &str) {
        let store = LamStore::default();
        let script = fs::read_to_string(path).unwrap();
        let e = EvalBuilder::new(script.into()).set_store(store).build();
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
        let e = EvalBuilder::new(Cow::Borrowed(&script))
            .set_input(Some(input.as_bytes()))
            .set_store(store)
            .build();
        let res = e.evaluate().expect(&script);
        assert_eq!(expected, res.result);
    }

    #[test]
    fn evaluate_infinite_loop() {
        let timeout = Duration::from_secs(1);

        let e = EvalBuilder::new(r#"while true do end"#.into())
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
        let e = EvalBuilder::new(script.into()).build();
        let res = e.evaluate().expect(script);
        assert_eq!(expected, res.result.to_string());
    }

    #[test]
    fn reevaluate() {
        let input = "foo\nbar";
        let script = r#"return require('@lam'):read('*l')"#;
        let e = EvalBuilder::new(script.into())
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
        let e = EvalBuilder::new(script.into()).build();
        let res = e.evaluate().expect(script);
        assert_eq!(expected, res.result.to_string());
    }

    #[test]
    fn syntax_error() {
        let script = "ret true"; // code with syntax error
        let e = EvalBuilder::new(script.into()).build();
        assert!(e.evaluate().is_err());
    }
}
