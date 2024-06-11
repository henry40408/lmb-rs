use bat::{
    assets::HighlightingAssets,
    controller::Controller,
    input::Input,
    style::{StyleComponent, StyleComponents},
};
use console::Term;
use mlua::{prelude::*, Compiler};
use parking_lot::Mutex;
use serde_json::Value;
use std::{
    fmt::Write,
    io::{BufReader, Read},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tracing::{debug, trace_span};

use crate::{LmbInput, LmbResult, LmbState, LmbStore, LuaBinding, PrintOptions, DEFAULT_TIMEOUT};

/// Evaluation builder.
pub struct EvaluationBuilder<R>
where
    R: Read,
{
    /// Function input, such as anything that implements [`std::io::Read`].
    pub input: Arc<Mutex<BufReader<R>>>,
    /// Function name. Might be `-` when script comes from standard input or file name.
    pub name: Option<String>,
    /// Lua script in plain text.
    pub script: String,
    /// Store that persists data across each execution.
    pub store: Option<LmbStore>,
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
    /// use lmb::*;
    /// let _ = EvaluationBuilder::new("", empty());
    /// ```
    pub fn new<S: AsRef<str>>(script: S, input: R) -> Self {
        let input = Arc::new(Mutex::new(BufReader::new(input)));
        Self {
            input,
            name: None,
            script: script.as_ref().to_string(),
            store: None,
            timeout: None,
        }
    }

    /// Build the evaluation with a [`std::io::BufReader`].
    ///
    /// ```rust
    /// # use std::{io::{empty, BufReader}, sync::Arc};
    /// # use parking_lot::Mutex;
    /// use lmb::*;
    /// let input = Arc::new(Mutex::new(BufReader::new(empty())));
    /// let _ = EvaluationBuilder::with_reader("", input);
    /// ```
    pub fn with_reader<S: AsRef<str>>(script: S, input: Arc<Mutex<BufReader<R>>>) -> Self {
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
    /// use lmb::*;
    /// let _ = EvaluationBuilder::new("", empty()).default_store();
    /// ```
    pub fn default_store(mut self) -> Self {
        self.store = Some(LmbStore::default());
        self
    }

    /// Name the function for debugging and/or verbosity.
    ///
    /// ```rust
    /// # use std::io::empty;
    /// use lmb::*;
    /// let _ = EvaluationBuilder::new("", empty()).name("script");
    /// ```
    pub fn name<S: AsRef<str>>(mut self, name: S) -> Self {
        self.name = Some(name.as_ref().to_string());
        self
    }

    /// Attach a store to the function.
    ///
    /// ```rust
    /// # use std::io::empty;
    /// use lmb::*;
    /// let store = LmbStore::default();
    /// let _ = EvaluationBuilder::new("", empty()).store(store);
    /// ```
    pub fn store(mut self, store: LmbStore) -> Self {
        self.store = Some(store);
        self
    }

    /// Set or unset execution timeout.
    ///
    /// ```rust
    /// # use std::{io::empty, time::Duration};
    /// use lmb::*;
    /// let timeout = Duration::from_secs(30);
    /// let _ = EvaluationBuilder::new("", empty()).timeout(Some(timeout));
    /// ```
    pub fn timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }

    /// Build the [`Evaluation`] for execution.
    /// It will compile Lua script into bytecodes for better performance.
    ///
    /// <div class="warning">However, this function won't check syntax of Lua script.</div>
    ///
    /// The syntax of Lua script could be checked with [`crate::LuaCheck`].
    ///
    /// ```rust
    /// # use std::io::empty;
    /// # use serde_json::json;
    /// use lmb::*;
    /// let e = EvaluationBuilder::new("return true", empty()).build();
    /// let res = e.evaluate().unwrap();
    /// assert_eq!(json!(true), res.payload);
    /// ```
    pub fn build(self) -> Arc<Evaluation<R>> {
        let vm = Lua::new();
        vm.sandbox(true).expect("failed to enable sandbox");

        let compiled = {
            let compiler = Compiler::new();
            let _s = trace_span!("compile_script").entered();
            compiler.compile(&self.script)
        };
        LuaBinding::register(&vm, self.input.clone(), self.store.clone(), None)
            .expect("failed to initalize the binding");
        Arc::new(Evaluation {
            compiled,
            input: self.input,
            name: self.name.unwrap_or_default(),
            script: self.script,
            store: self.store,
            timeout: self.timeout.unwrap_or(DEFAULT_TIMEOUT),
            vm,
        })
    }
}

/// Solution obtained by the function.
pub struct Solution<R>
where
    for<'lua> R: 'lua + Read,
{
    /// Execution duration.
    pub duration: Duration,
    /// Container of function that obtained the solution.
    pub evaluation: Arc<Evaluation<R>>,
    /// Max memory usage in bytes.
    pub max_memory: usize,
    /// Result returned by the function.
    pub payload: Value,
}

impl<R> Solution<R>
where
    for<'lua> R: 'lua + Read,
{
    /// Render the solution.
    pub fn render<W>(&self, mut f: W, json: bool) -> LmbResult<()>
    where
        W: Write,
    {
        if json {
            let res = serde_json::to_string(&self.payload)?;
            Ok(write!(f, "{}", res)?)
        } else {
            match &self.payload {
                Value::String(s) => Ok(write!(f, "{}", s)?),
                _ => Ok(write!(f, "{}", self.payload)?),
            }
        }
    }
}

/// A container that holds the compiled function and input for evaluation.
pub struct Evaluation<R>
where
    for<'lua> R: 'lua + Read,
{
    /// Source code of script.
    pub script: String,
    /// Name.
    pub name: String,
    compiled: Vec<u8>,
    input: LmbInput<R>,
    store: Option<LmbStore>,
    timeout: Duration,
    vm: Lua,
}

impl<R> Evaluation<R>
where
    for<'lua> R: 'lua + Read + Send,
{
    /// Evaluate the function and return a [`crate::Solution`] as result.
    ///
    /// ```rust
    /// # use std::io::empty;
    /// # use serde_json::json;
    /// use lmb::*;
    /// let e = EvaluationBuilder::new("return 1+1", empty()).build();
    /// let res = e.evaluate().unwrap();
    /// assert_eq!(json!(2), res.payload);
    /// ```
    pub fn evaluate(self: &Arc<Self>) -> LmbResult<Solution<R>> {
        self.do_evaluate(None)
    }

    /// Evaluate the function with a state.
    ///
    /// ```rust
    /// # use std::{io::empty, sync::Arc};
    /// # use serde_json::json;
    /// use lmb::*;
    /// let e = EvaluationBuilder::new("return 1+1", empty()).build();
    /// let state = Arc::new(LmbState::new());
    /// state.insert(LmbStateKey::from("bool"), true.into());
    /// let res = e.evaluate_with_state(state).unwrap();
    /// assert_eq!(json!(2), res.payload);
    /// ```
    pub fn evaluate_with_state(self: &Arc<Self>, state: Arc<LmbState>) -> LmbResult<Solution<R>> {
        self.do_evaluate(Some(state))
    }

    /// Render the script.
    pub fn write_script<W>(&self, mut f: W, options: &PrintOptions) -> LmbResult<bool>
    where
        W: Write,
    {
        let components = &[StyleComponent::Grid, StyleComponent::LineNumbers];
        let style_components = StyleComponents::new(components);
        let mut config = bat::config::Config {
            colored_output: !options.no_color,
            language: Some("lua"),
            style_components,
            true_color: true,
            // required to print line numbers
            term_width: Term::stdout().size().1 as usize,
            ..Default::default()
        };
        if let Some(theme) = &options.theme {
            config.theme.clone_from(theme);
        }
        let assets = HighlightingAssets::from_binary();
        let reader = Box::new(self.script.as_bytes());
        let inputs = vec![Input::from_reader(reader)];
        let controller = Controller::new(&config, &assets);
        Ok(controller.run(inputs, Some(&mut f))?)
    }

    /// Replace the function input after the container is built.
    ///
    /// ```rust
    /// # use std::io::{BufReader, Cursor, empty};
    /// # use serde_json::json;
    /// use lmb::*;
    ///
    /// let script = "return io.read('*a')";
    /// let mut e = EvaluationBuilder::new(script, Cursor::new("1")).build();
    ///
    /// let r = e.evaluate().unwrap();
    /// assert_eq!(json!("1"), r.payload);
    ///
    /// e.set_input(Cursor::new("2"));
    ///
    /// let r = e.evaluate().unwrap();
    /// assert_eq!(json!("2"), r.payload);
    /// ```
    pub fn set_input(self: &Arc<Self>, input: R) {
        *self.input.lock() = BufReader::new(input);
    }

    fn do_evaluate(self: &Arc<Self>, state: Option<Arc<LmbState>>) -> LmbResult<Solution<R>> {
        let vm = &self.vm;
        if state.is_some() {
            LuaBinding::register(vm, self.input.clone(), self.store.clone(), state)?;
        }

        let max_memory = Arc::new(AtomicUsize::new(0));
        let timeout = self.timeout;

        let start = Instant::now();
        self.vm.set_interrupt({
            let max_memory = Arc::clone(&max_memory);
            move |vm| {
                let used_memory = vm.used_memory();
                max_memory.fetch_max(used_memory, Ordering::Relaxed);
                if start.elapsed() > timeout {
                    vm.remove_interrupt();
                    return Err(mlua::Error::runtime("timeout"));
                }
                Ok(LuaVmState::Continue)
            }
        });

        let script_name = &self.name;
        let chunk = vm.load(&self.compiled).set_name(script_name);

        let _s = trace_span!("evaluate").entered();
        let result = vm.from_value(chunk.eval()?)?;

        let duration = start.elapsed();
        let max_memory = max_memory.load(Ordering::Acquire);
        debug!(?duration, %script_name, ?max_memory, "script evaluated");
        Ok(Solution {
            duration,
            evaluation: self.clone(),
            max_memory,
            payload: result,
        })
    }
}

#[cfg(test)]
mod tests {
    use parking_lot::Mutex;
    use serde_json::{json, Value};
    use std::{
        fs,
        io::{empty, BufReader},
        sync::Arc,
        time::{Duration, Instant},
    };
    use test_case::test_case;

    use crate::{EvaluationBuilder, LmbState, LmbStateKey};

    #[test_case("./lua-examples/error.lua")]
    fn error_in_script(path: &str) {
        let script = fs::read_to_string(path).unwrap();
        let e = EvaluationBuilder::new(script, empty()).build();
        assert!(e.evaluate().is_err());
    }

    #[test_case("algebra.lua", "2", 4.into())]
    #[test_case("count-bytes.lua", "A", json!({ "65": 1 }))]
    #[test_case("hello.lua", "", json!(null))]
    #[test_case("input.lua", "lua", json!(null))]
    #[test_case("read-unicode.lua", "你好，世界", "你好".into())]
    #[test_case("return-table.lua", "123", json!({ "bool": true, "num": 1.23, "str": "hello" }))]
    #[test_case("store.lua", "", 1.into())]
    fn evaluate_examples(filename: &str, input: &'static str, expected: Value) {
        let script = fs::read_to_string(format!("./lua-examples/{filename}")).unwrap();
        let e = EvaluationBuilder::new(&script, input.as_bytes())
            .default_store()
            .build();
        let res = e.evaluate().unwrap();
        assert_eq!(expected, res.payload);
    }

    #[test]
    fn evaluate_infinite_loop() {
        let timer = Instant::now();
        let timeout = Duration::from_millis(100);

        let e = EvaluationBuilder::new(r#"while true do end"#, empty())
            .timeout(Some(timeout))
            .build();
        let res = e.evaluate();
        assert!(res.is_err());

        let elapsed = timer.elapsed().as_millis();
        assert!(elapsed < 500, "actual elapsed {elapsed:?}"); // 500% error
    }

    #[test_case("return 1+1", json!(2))]
    #[test_case("return 'a'..1", json!("a1"))]
    #[test_case("return require('@lmb')._VERSION", json!(env!("APP_VERSION")))]
    fn evaluate_scripts(script: &str, expected: Value) {
        let e = EvaluationBuilder::new(script, empty()).build();
        let res = e.evaluate().unwrap();
        assert_eq!(expected, res.payload);
    }

    #[test]
    fn reevaluate() {
        let input = "foo\nbar";
        let script = "return io.read('*l')";
        let e = EvaluationBuilder::new(script, input.as_bytes()).build();

        let res = e.evaluate().unwrap();
        assert_eq!(json!("foo"), res.payload);

        let res = e.evaluate().unwrap();
        assert_eq!(json!("bar"), res.payload);
    }

    #[test]
    fn render_solution() {
        let script = "return 1+1";
        let e = EvaluationBuilder::new(script, empty()).build();
        let solution = e.evaluate().unwrap();
        let mut buf = String::new();
        solution.render(&mut buf, false).unwrap();
        assert_eq!("2", buf);
    }

    #[test]
    fn replace_input() {
        let script = "return io.read('*a')";
        let e = EvaluationBuilder::new(script, &b"0"[..]).build();

        let res = e.evaluate().unwrap();
        assert_eq!(json!("0"), res.payload);

        e.set_input(&b"1"[..]);

        let res = e.evaluate().unwrap();
        assert_eq!(json!("1"), res.payload);
    }

    #[test]
    fn syntax_error() {
        let script = "ret true"; // code with syntax error
        let e = EvaluationBuilder::new(script, empty()).build();
        assert!(e.evaluate().is_err());
    }

    #[test]
    fn with_bufreader() {
        let input = Arc::new(Mutex::new(BufReader::new(empty())));
        let e = EvaluationBuilder::with_reader("return nil", input.clone()).build();
        let res = e.evaluate().unwrap();
        assert_eq!(json!(null), res.payload);
        let _input = input;
    }

    #[test]
    fn with_state() {
        let e = EvaluationBuilder::new(r#"return require("@lmb").request"#, empty()).build();
        let state = Arc::new(LmbState::new());
        state.insert(LmbStateKey::Request, 1.into());
        {
            let res = e.evaluate_with_state(state.clone()).unwrap();
            assert_eq!(json!(1), res.payload);
        }
        state.insert(LmbStateKey::Request, 2.into());
        {
            let res = e.evaluate_with_state(state.clone()).unwrap();
            assert_eq!(json!(2), res.payload);
        }
    }
}
