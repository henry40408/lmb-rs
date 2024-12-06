use bat::{
    assets::HighlightingAssets,
    controller::Controller,
    input::Input as BatInput,
    style::{StyleComponent, StyleComponents},
};
use bon::{bon, builder, Builder};
use chrono::Utc;
use console::Term;
use mlua::{prelude::*, Compiler};
use parking_lot::Mutex;
use serde_json::Value;
use std::{
    fmt::Write,
    io::{stdout, BufReader, IsTerminal as _, Read},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use tracing::{debug, error, trace_span, warn};

use crate::{bind_vm, Input, PrintOptions, Result, ScheduleOptions, State, Store, DEFAULT_TIMEOUT};

/// Solution obtained by the function.
#[derive(Builder, Debug)]
pub struct Solution<R>
where
    for<'lua> R: 'lua + Read,
{
    /// Evaluation.
    #[builder(start_fn)]
    pub evaluation: Arc<Evaluation<R>>,
    /// Duration.
    pub duration: Duration,
    /// Max memory usage in bytes.
    pub max_memory_usage: usize,
    /// Payload returned by the script.
    pub payload: Value,
}

#[bon]
impl<R> Solution<R>
where
    for<'lua> R: 'lua + Read,
{
    /// Render the solution.
    #[builder]
    pub fn write<W>(&self, #[builder(start_fn)] mut f: W, json: Option<bool>) -> Result<()>
    where
        W: Write,
    {
        let json = json.unwrap_or_else(|| false);
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

/// Container holding the compiled function and input for evaluation.
#[derive(Debug)]
pub struct Evaluation<R>
where
    for<'lua> R: 'lua + Read,
{
    /// Input.
    input: Input<R>,
    /// Name of script.
    name: Option<String>,
    /// Script.
    script: String,
    /// Store.
    store: Option<Store>,
    /// Timeout.
    timeout: Option<Duration>,
    /// Lua code compiled by [`mlua::Compiler`].
    compiled: Vec<u8>,
    /// Lua virtual machine.
    vm: Lua,
}

#[bon]
impl<R> Evaluation<R>
where
    for<'lua> R: 'lua + Read + Send,
{
    /// Build evaluation.
    #[builder]
    pub fn new(
        #[builder(into, start_fn)] script: String,
        #[builder(start_fn)] input: R,
        name: Option<String>,
        store: Option<Store>,
        timeout: Option<Duration>,
    ) -> Result<Arc<Evaluation<R>>> {
        let compiled = {
            let _s = trace_span!("compile_script").entered();
            let compiler = Compiler::new();
            compiler.compile(&script)?
        };
        let vm = Lua::new();
        vm.sandbox(true)?;
        let input = Arc::new(Mutex::new(BufReader::new(input)));
        bind_vm(&vm, input.clone())
            .maybe_store(store.clone())
            .call()?;
        Ok(Arc::new(Evaluation {
            input,
            name,
            script,
            store,
            timeout,
            compiled,
            vm,
        }))
    }

    /// Evaluate the function with a state.
    ///
    /// ```rust
    /// # use std::{io::empty, sync::Arc};
    /// # use serde_json::json;
    /// use lmb::*;
    ///
    /// # fn main() -> Result<()> {
    /// let e = Evaluation::builder("return 1+1", empty()).build().unwrap();
    /// let state = Arc::new(State::new());
    /// state.insert(StateKey::from("bool"), true.into());
    /// let res = e.evaluate().state(state).call()?;
    /// assert_eq!(json!(2), res.payload);
    /// # Ok(())
    /// # }
    /// ```
    #[builder]
    pub fn evaluate(self: &Arc<Self>, state: Option<Arc<State>>) -> Result<Solution<R>> {
        if state.is_some() {
            bind_vm(&self.vm, self.input.clone())
                .maybe_store(self.store.clone())
                .maybe_state(state)
                .call()?;
        }

        let timeout = self.timeout.unwrap_or_else(|| DEFAULT_TIMEOUT);
        let max_memory = Arc::new(AtomicUsize::new(0));

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
        let chunk = self.vm.load(&self.compiled);
        let chunk = match &self.name {
            Some(name) => chunk.set_name(name),
            None => chunk,
        };

        let _s = trace_span!("evaluate").entered();
        let result = self.vm.from_value(chunk.eval()?)?;

        let duration = start.elapsed();
        let max_memory = max_memory.load(Ordering::Acquire);
        debug!(?duration, ?script_name, ?max_memory, "script evaluated");
        let solution = Solution::builder(self.clone())
            .duration(duration)
            .max_memory_usage(max_memory)
            .payload(result)
            .build();
        Ok(solution)
    }

    /// Get the name
    pub fn name(&self) -> &str {
        self.name.as_deref().unwrap_or_else(|| "")
    }

    /// Get the script
    pub fn script(&self) -> &str {
        self.script.as_ref()
    }

    /// Schedule the script.
    pub fn schedule(self: &Arc<Self>, options: &ScheduleOptions) {
        let bail = options.bail;
        debug!(bail, "script scheduled");
        let mut error_count = 0usize;
        loop {
            let now = Utc::now();
            if let Some(next) = options.schedule.upcoming(Utc).take(1).next() {
                debug!(%next, "next run");
                let elapsed = next - now;
                thread::sleep(elapsed.to_std().expect("failed to fetch next schedule"));
                if let Err(err) = self.clone().evaluate().call() {
                    warn!(?err, "failed to evaluate");
                    if bail > 0 {
                        debug!(bail, error_count, "check bail threshold");
                        error_count += 1;
                        if error_count == bail {
                            error!("bail because threshold reached");
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Replace the input
    pub fn set_input(self: &Arc<Self>, input: R) {
        *self.input.lock() = BufReader::new(input);
    }

    /// Render the script.
    ///
    /// ```rust
    /// # use std::io::empty;
    /// use lmb::*;
    ///
    /// # fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// let script = "return 1";
    /// let e = Evaluation::builder(script, empty()).build().unwrap();
    ///
    /// let mut buf = String::new();
    /// let print_options = PrintOptions::builder().no_color(true).build();
    /// e.write_script(&mut buf, &print_options)?;
    /// assert!(buf.contains("return 1"));
    /// # Ok(())
    /// # }
    /// ```
    pub fn write_script<W>(&self, mut f: W, options: &PrintOptions) -> Result<bool>
    where
        W: Write,
    {
        let (style_components, colored_output) = if stdout().is_terminal() {
            let components = &[StyleComponent::Grid, StyleComponent::LineNumbers];
            (StyleComponents::new(components), !options.no_color)
        } else {
            (StyleComponents::new(&[]), false)
        };
        let mut config = bat::config::Config {
            colored_output,
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
        let inputs = vec![BatInput::from_reader(reader)];
        let controller = Controller::new(&config, &assets);
        Ok(controller.run(inputs, Some(&mut f))?)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};
    use std::{
        fs,
        io::empty,
        sync::Arc,
        time::{Duration, Instant},
    };
    use test_case::test_case;

    use crate::{Evaluation, State, StateKey, Store};

    #[test_case("./lua-examples/error.lua")]
    fn error_in_script(path: &str) {
        let script = fs::read_to_string(path).unwrap();
        let e = Evaluation::builder(script, empty()).build().unwrap();
        assert!(e.evaluate().call().is_err());
    }

    #[test_case("algebra.lua", "2", 4.into())]
    #[test_case("count-bytes.lua", "A", json!({ "65": 1 }))]
    #[test_case("hello.lua", "", json!(null))]
    #[test_case("input.lua", "lua", json!(null))]
    #[test_case("read-unicode.lua", "你好，世界", "你好".into())]
    #[test_case("return-table.lua", "123", json!({ "bool": true, "num": 1.23, "str": "hello" }))]
    #[test_case("store.lua", "", json!([1]))]
    fn evaluate_examples(filename: &str, input: &'static str, expected: Value) {
        let script = fs::read_to_string(format!("./lua-examples/{filename}")).unwrap();
        let store = Store::default();
        let e = Evaluation::builder(&script, input.as_bytes())
            .store(store)
            .build()
            .unwrap();
        let res = e.evaluate().call().unwrap();
        assert_eq!(expected, res.payload);
    }

    #[test]
    fn evaluate_infinite_loop() {
        let timer = Instant::now();
        let timeout = Duration::from_millis(100);
        let e = Evaluation::builder(r#"while true do end"#, empty())
            .timeout(timeout)
            .build()
            .unwrap();
        let res = e.evaluate().call();
        assert!(res.is_err());

        let elapsed = timer.elapsed().as_millis();
        assert!(elapsed < 500, "actual elapsed {elapsed:?}"); // 500% error
    }

    #[test_case("return 1+1", json!(2))]
    #[test_case("return 'a'..1", json!("a1"))]
    #[test_case("return require('@lmb')._VERSION", json!(env!("APP_VERSION")))]
    fn evaluate_scripts(script: &str, expected: Value) {
        let e = Evaluation::builder(script, empty()).build().unwrap();
        let res = e.evaluate().call().unwrap();
        assert_eq!(expected, res.payload);
    }

    #[test]
    fn reevaluate() {
        let input = "foo\nbar";
        let script = "return io.read('*l')";
        let e = Evaluation::builder(script, input.as_bytes())
            .build()
            .unwrap();

        let res = e.evaluate().call().unwrap();
        assert_eq!(json!("foo"), res.payload);

        let res = e.evaluate().call().unwrap();
        assert_eq!(json!("bar"), res.payload);
    }

    #[test]
    fn replace_input() {
        let script = "return io.read('*a')";
        let e = Evaluation::builder(script, &b"0"[..]).build().unwrap();

        let res = e.evaluate().call().unwrap();
        assert_eq!(json!("0"), res.payload);

        e.set_input(&b"1"[..]);

        let res = e.evaluate().call().unwrap();
        assert_eq!(json!("1"), res.payload);
    }

    #[test]
    fn syntax_error() {
        let script = "ret true"; // code with syntax error
        let e = Evaluation::builder(script, empty()).build();
        assert!(e.is_err());
    }

    #[test]
    fn with_state() {
        let e = Evaluation::builder(r#"return require("@lmb").request"#, empty())
            .build()
            .unwrap();
        let state = Arc::new(State::new());
        state.insert(StateKey::Request, 1.into());
        {
            let res = e.evaluate().state(state.clone()).call().unwrap();
            assert_eq!(json!(1), res.payload);
        }
        state.insert(StateKey::Request, 2.into());
        {
            let res = e.evaluate().state(state.clone()).call().unwrap();
            assert_eq!(json!(2), res.payload);
        }
    }

    #[test]
    fn write_solution() {
        let script = "return 1+1";
        let e = Evaluation::builder(script, empty()).build().unwrap();
        let solution = e.evaluate().call().unwrap();
        let mut buf = String::new();
        solution.write(&mut buf).call().unwrap();
        assert_eq!("2", buf);
    }
}
