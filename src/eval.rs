use mlua::{prelude::*, Compiler};
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

use crate::{LamInput, LamResult, LamState, LamStore, LamValue, LuaLam};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Evaluation builder.
#[derive(Default)]
pub struct EvaluationBuilder<R>
where
    R: Read,
{
    /// Function input, such as anything that implements [`std::io::Read`].
    pub input: R,
    /// Function name. Might be `(stdin)` or file name.
    pub name: Option<String>,
    /// Lua script in plain text.
    pub script: String,
    /// Store that persists data across each execution.
    pub store: Option<LamStore>,
    /// Execution timeout.
    pub timeout: Option<Duration>,
}

impl<R> EvaluationBuilder<R>
where
    for<'lua> R: 'lua + Read + Send,
{
    /// Create a builder.
    ///
    /// ```rust
    /// # use std::io::empty;
    /// use lam::*;
    /// let _ = EvaluationBuilder::new("", empty());
    /// ```
    pub fn new<S: AsRef<str>>(script: S, input: R) -> Self {
        Self {
            input,
            name: None,
            script: script.as_ref().to_string(),
            store: None,
            timeout: None,
        }
    }

    /// Attach an in-memory store.
    /// <div class="warning">Data will be lost after the program finishes.</div>
    ///
    /// ```rust
    /// # use std::io::empty;
    /// use lam::*;
    /// let _ = EvaluationBuilder::new("", empty()).with_default_store();
    /// ```
    pub fn with_default_store(mut self) -> Self {
        self.store = Some(LamStore::default());
        self
    }

    /// Name the function for debugging and/or verbosity.
    ///
    /// ```rust
    /// # use std::io::empty;
    /// use lam::*;
    /// let _ = EvaluationBuilder::new("", empty()).with_name("script");
    /// ```
    pub fn with_name<S: AsRef<str>>(mut self, name: S) -> Self {
        self.name = Some(name.as_ref().to_string());
        self
    }

    /// Set or unset execution timeout.
    ///
    /// ```rust
    /// # use std::{io::empty, time::Duration};
    /// use lam::*;
    /// let timeout = Duration::from_secs(30);
    /// let _ = EvaluationBuilder::new("", empty()).with_timeout(Some(timeout));
    /// ```
    pub fn with_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }

    /// Attach a store to the function.
    ///
    /// ```rust
    /// # use std::io::empty;
    /// use lam::*;
    /// let store = LamStore::default();
    /// let _ = EvaluationBuilder::new("", empty()).with_store(store);
    /// ```
    pub fn with_store(mut self, store: LamStore) -> Self {
        self.store = Some(store);
        self
    }

    /// Build the [`Evaluation`] for execution.
    /// It will compile Lua script into bytecodes for better performance.
    ///
    /// <div class="warning">However, this function won't check syntax of Lua script.</div>
    ///
    /// The syntax of Lua script could be checked with [`check_syntax`].
    ///
    /// ```rust
    /// # use std::io::empty;
    /// use lam::*;
    /// let e = EvaluationBuilder::new("return true", empty()).build();
    /// let res = e.evaluate().unwrap();
    /// assert_eq!(LamValue::from(true), res.result);
    /// ```
    pub fn build(self) -> Evaluation<R> {
        let vm = Lua::new();
        vm.sandbox(true).expect("failed to enable sandbox");

        let compiled = {
            let compiler = Compiler::new();
            let _ = trace_span!("compile script").entered();
            compiler.compile(&self.script)
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
pub struct EvaluationResult {
    /// Execution duration.
    pub duration: Duration,
    /// Max memory usage in bytes.
    pub max_memory: usize,
    /// Result returned by the function.
    pub result: LamValue,
}

/// A container that holds the compiled function and input for evaluation.
pub struct Evaluation<R>
where
    for<'lua> R: 'lua + Read,
{
    compiled: Vec<u8>,
    input: LamInput<BufReader<R>>,
    name: String,
    store: Option<LamStore>,
    timeout: Duration,
    vm: Lua,
}

impl<R> Evaluation<R>
where
    for<'lua> R: 'lua + Read + Send,
{
    /// Evaluate the function and return a [`EvaluationResult`] as result.
    ///
    /// ```rust
    /// # use std::io::empty;
    /// use lam::*;
    /// let e = EvaluationBuilder::new("return 1+1", empty()).build();
    /// let res = e.evaluate().unwrap();
    /// assert_eq!(LamValue::from(2), res.result);
    /// ```
    pub fn evaluate(&self) -> LamResult<EvaluationResult> {
        self.do_evaluate(None)
    }

    /// Evaluate the function with a state.
    ///
    /// ```rust
    /// # use std::io::empty;
    /// use lam::*;
    /// let e = EvaluationBuilder::new("return 1+1", empty()).build();
    /// let state = LamState::new();
    /// state.insert(LamStateKey::from("bool"), true.into());
    /// let res = e.evaluate_with_state(state).unwrap();
    /// assert_eq!(LamValue::from(2), res.result);
    /// ```
    pub fn evaluate_with_state(&self, state: LamState) -> LamResult<EvaluationResult> {
        self.do_evaluate(Some(state))
    }

    /// Replace the function input after the container is built.
    ///
    /// ```rust
    /// # use std::io::{BufReader, Cursor, empty};
    /// use lam::*;
    ///
    /// let script = "return require('@lam'):read('*a')";
    /// let mut e = EvaluationBuilder::new(script, Cursor::new("1")).build();
    ///
    /// let r = e.evaluate().unwrap();
    /// assert_eq!(LamValue::from("1"), r.result);
    ///
    /// e.set_input(Cursor::new("2"));
    ///
    /// let r = e.evaluate().unwrap();
    /// assert_eq!(LamValue::from("2"), r.result);
    /// ```
    pub fn set_input(&mut self, input: R) {
        self.input = Arc::new(Mutex::new(BufReader::new(input)));
    }

    fn do_evaluate(&self, state: Option<LamState>) -> LamResult<EvaluationResult> {
        let vm = &self.vm;
        LuaLam::register(vm, self.input.clone(), self.store.clone(), state)?;

        let max_memory = Arc::new(AtomicUsize::new(0));
        let timeout = self.timeout;

        let start = Instant::now();
        self.vm.set_interrupt({
            let max_memory = Arc::clone(&max_memory);
            move |vm| {
                let used_memory = vm.used_memory();
                max_memory.fetch_max(used_memory, Ordering::SeqCst);
                if start.elapsed() > timeout {
                    vm.remove_interrupt();
                    return Err(mlua::Error::runtime("timeout"));
                }
                Ok(LuaVmState::Continue)
            }
        });

        let name = &self.name;
        let chunk = vm.load(&self.compiled).set_name(name);

        let _ = trace_span!("evaluate", name).entered();
        let result = chunk.eval()?;

        let duration = start.elapsed();
        let max_memory = max_memory.load(Ordering::SeqCst);
        debug!(?duration, name, ?max_memory, "evaluated");
        Ok(EvaluationResult {
            duration,
            max_memory,
            result,
        })
    }
}

#[cfg(test)]
mod tests {
    use maplit::hashmap;
    use std::{
        fs,
        io::empty,
        time::{Duration, Instant},
    };
    use test_case::test_case;

    use crate::{EvaluationBuilder, LamValue};

    #[test_case("./lua-examples/error.lua")]
    fn error_in_script(path: &str) {
        let script = fs::read_to_string(path).unwrap();
        let e = EvaluationBuilder::new(script, empty()).build();
        assert!(e.evaluate().is_err());
    }

    #[test_case("hello.lua", "", LamValue::None)]
    #[test_case("input.lua", "lua", LamValue::None)]
    #[test_case("algebra.lua", "2", 4.into())]
    #[test_case("store.lua", "", 1.into())]
    #[test_case("count-bytes.lua", "A", hashmap!{ "65" => 1.into() }.into())]
    #[test_case("return-table.lua", "123", hashmap!{
        "bool" => true.into(),
        "num" => 1.23.into(),
        "str" => "hello".into()
    }.into())]
    #[test_case("read-unicode.lua", "你好，世界", "你好".into())]
    fn evaluate_examples(filename: &str, input: &'static str, expected: LamValue) {
        let script = fs::read_to_string(format!("./lua-examples/{filename}")).unwrap();
        let e = EvaluationBuilder::new(&script, input.as_bytes())
            .with_default_store()
            .build();
        let res = e.evaluate().expect(&script);
        assert_eq!(expected, res.result);
    }

    #[test]
    fn evaluate_infinite_loop() {
        let timer = Instant::now();
        let timeout = Duration::from_millis(100);

        let e = EvaluationBuilder::new(r#"while true do end"#, empty())
            .with_timeout(Some(timeout))
            .build();
        let res = e.evaluate();
        assert!(res.is_err());

        let elapsed = timer.elapsed().as_millis();
        assert!(elapsed < 300, "actual elapsed {elapsed:?}"); // 300% error
    }

    #[test_case("return 1+1", "2")]
    #[test_case("return 'a'..1", "a1")]
    #[test_case("return require('@lam')._VERSION", env!("APP_VERSION"))]
    fn evaluate_scripts(script: &str, expected: &str) {
        let e = EvaluationBuilder::new(script, empty()).build();
        let res = e.evaluate().expect(script);
        assert_eq!(expected, res.result.to_string());
    }

    #[test_case(r#"return {a=true,b=1.23,c="hello"}"#)]
    #[test_case(r#"return {true,1.23,"hello"}"#)]
    fn collection_to_string(script: &str) {
        let e = EvaluationBuilder::new(script, empty()).build();
        let res = e.evaluate().expect(script);
        let actual = res.result.to_string();
        assert!(actual.starts_with("table: 0x"));
    }

    #[test]
    fn reevaluate() {
        let input = "foo\nbar";
        let script = "return require('@lam'):read('*l')";
        let e = EvaluationBuilder::new(script, input.as_bytes()).build();

        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::from("foo"), res.result);

        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::from("bar"), res.result);
    }

    #[test_case(r#""#, "")]
    #[test_case(r#"return nil"#, "")]
    #[test_case(r#"return true"#, "true")]
    #[test_case(r#"return false"#, "false")]
    #[test_case(r#"return 1"#, "1")]
    #[test_case(r#"return 1.23"#, "1.23")]
    #[test_case(r#"return 'hello'"#, "hello")]
    fn return_to_string(script: &str, expected: &str) {
        let e = EvaluationBuilder::new(script, empty()).build();
        let res = e.evaluate().expect(script);
        assert_eq!(expected, res.result.to_string());
    }

    #[test]
    fn syntax_error() {
        let script = "ret true"; // code with syntax error
        let e = EvaluationBuilder::new(script, empty()).build();
        assert!(e.evaluate().is_err());
    }

    #[test]
    fn replace_input() {
        let script = "return require('@lam'):read('*a')";
        let mut e = EvaluationBuilder::new(script, &b"0"[..]).build();

        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::from("0"), res.result);

        e.set_input(&b"1"[..]);

        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::from("1"), res.result);
    }
}
