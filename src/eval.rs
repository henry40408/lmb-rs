use crate::*;
use mlua::{prelude::*, Compiler};
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

/// Evaluation builder.
#[derive(Default)]
pub struct EvalBuilder<'a, R>
where
    for<'lua> R: Read + 'lua,
{
    /// Function input, such as anything that implements [`std::io::Read`].
    pub input: R,
    /// Function name. Might be `(stdin)` or file name.
    pub name: Option<Cow<'a, str>>,
    /// Lua script in plain text.
    pub script: Cow<'a, str>,
    /// Store that persists data across each execution.
    pub store: Option<LamStore>,
    /// Execution timeout.
    pub timeout: Option<Duration>,
}

impl<'a, R> EvalBuilder<'a, R>
where
    for<'lua> R: Read + 'lua,
{
    /// Create a builder without input.
    ///
    /// ```rust
    /// use lam::*;
    /// let _ = EvalBuilder::new("".into(), &b""[..]);
    /// ```
    pub fn new(script: Cow<'a, str>, input: R) -> Self {
        Self {
            input,
            name: None,
            script,
            store: None,
            timeout: None,
        }
    }

    /// Attach an in-memory store.
    /// <div class="warning">Data will be lost after the program finishes.</div>
    ///
    /// ```rust
    /// use lam::*;
    /// let _ = EvalBuilder::new("".into(), &b""[..]).with_default_store();
    /// ```
    pub fn with_default_store(mut self) -> Self {
        self.store = Some(LamStore::default());
        self
    }

    /// Name the function for verbosity.
    ///
    /// ```rust
    /// use lam::*;
    /// let _ = EvalBuilder::new("".into(), &b""[..]).with_name("script".into());
    /// ```
    pub fn with_name(mut self, name: Cow<'a, str>) -> Self {
        self.name = Some(name);
        self
    }

    /// Set or unset execution timeout.
    ///
    /// ```rust
    /// # use std::time::Duration;
    /// use lam::*;
    /// let timeout = Duration::from_secs(30);
    /// let _ = EvalBuilder::new("".into(), &b""[..]).with_timeout(Some(timeout));
    /// ```
    pub fn with_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }

    /// Attach a store to the function.
    ///
    /// ```rust
    /// use lam::*;
    /// let store = LamStore::default();
    /// let _ = EvalBuilder::new("".into(), &b""[..]).with_store(store);
    /// ```
    pub fn with_store(mut self, store: LamStore) -> Self {
        self.store = Some(store);
        self
    }

    /// Build the [`Evaluation`] for execution.
    /// It will compile Lua script into bytecodes for better performance.
    ///
    /// <div class="warning">This function doesn't check syntax of Lua script.</div>
    ///
    /// The syntax of Lua script could be checked with [`check_syntax`].
    ///
    /// ```rust
    /// use lam::*;
    /// let e = EvalBuilder::new("return true".into(), &b""[..]).build();
    /// let res = e.evaluate().unwrap();
    /// assert_eq!(LamValue::Boolean(true), res.result);
    /// ```
    pub fn build(self) -> Evaluation<'a, R> {
        let vm = Lua::new();
        vm.sandbox(true).expect("failed to enable sandbox");

        let compiled = {
            let compiler = Compiler::new();
            let _ = trace_span!("compile script").entered();
            compiler.compile(self.script.as_ref())
        };
        Evaluation {
            compiled,
            input: Arc::new(Mutex::new(BufReader::new(self.input))),
            name: self.name.unwrap_or_default(),
            store: self.store,
            timeout: self.timeout.unwrap_or(DEFAULT_TIMEOUT),
            vm,
        }
    }
}

/// Evaluation result.
#[derive(Debug)]
pub struct EvalResult {
    /// Execution duration.
    pub duration: Duration,
    /// Max memory usage in bytes.
    pub max_memory: usize,
    /// Result returned by the function.
    pub result: LamValue,
}

/// A container that holds the compiled function for execution.
pub struct Evaluation<'a, R>
where
    for<'lua> R: Read + 'lua,
{
    compiled: Vec<u8>,
    input: LamInput<R>,
    name: Cow<'a, str>,
    store: Option<LamStore>,
    timeout: Duration,
    vm: Lua,
}

impl<'a, R> Evaluation<'a, R>
where
    for<'lua> R: Read + 'lua,
{
    /// Evaluate the function and return a [`EvalResult`] as result.
    ///
    /// ```rust
    /// use lam::*;
    /// let e = EvalBuilder::new("return 1+1".into(), &b""[..]).build();
    /// let res = e.evaluate().unwrap();
    /// assert_eq!(LamValue::Number(2f64), res.result);
    /// ```
    pub fn evaluate(&self) -> LamResult<EvalResult> {
        let vm = &self.vm;
        LuaLam::register(vm, self.input.clone(), self.store.clone())?;

        let max_memory = Arc::new(AtomicUsize::new(0));
        let timeout = self.timeout;

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

        let chunk = vm.load(&self.compiled).set_name(self.name.as_ref());
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

    /// Replace the function input after the container is built.
    pub fn set_input(&mut self, input: R) {
        self.input = Arc::new(Mutex::new(BufReader::new(input)));
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
        let script = fs::read_to_string(path).unwrap();
        let e = EvalBuilder::new(script.into(), &b""[..]).build();
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
        let script = fs::read_to_string(format!("./lua-examples/{filename}")).unwrap();
        let e = EvalBuilder::new(Cow::Borrowed(&script), input.as_bytes())
            .with_default_store()
            .build();
        let res = e.evaluate().expect(&script);
        assert_eq!(expected, res.result);
    }

    #[test]
    fn evaluate_infinite_loop() {
        let timeout = Duration::from_secs(1);

        let e = EvalBuilder::new(r#"while true do end"#.into(), &b""[..])
            .with_timeout(Some(timeout))
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
        let e = EvalBuilder::new(script.into(), &b""[..]).build();
        let res = e.evaluate().expect(script);
        assert_eq!(expected, res.result.to_string());
    }

    #[test]
    fn reevaluate() {
        let input = "foo\nbar";
        let script = r#"return require('@lam'):read('*l')"#;
        let e = EvalBuilder::new(script.into(), input.as_bytes()).build();

        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::String("foo".into()), res.result);

        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::String("bar".into()), res.result);
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
        let e = EvalBuilder::new(script.into(), &b""[..]).build();
        let res = e.evaluate().expect(script);
        assert_eq!(expected, res.result.to_string());
    }

    #[test]
    fn syntax_error() {
        let script = "ret true"; // code with syntax error
        let e = EvalBuilder::new(script.into(), &b""[..]).build();
        assert!(e.evaluate().is_err());
    }

    #[test]
    fn replace_input() {
        let mut e = EvalBuilder::new("return require('@lam'):read('*a')".into(), &b"0"[..]).build();

        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::String("0".into()), res.result);

        e.set_input(&b"1"[..]);

        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::String("1".into()), res.result);
    }
}
