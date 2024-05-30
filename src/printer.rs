use std::fmt::Write;
use std::io::Read;

use bat::{
    assets::HighlightingAssets,
    config::Config,
    controller::Controller,
    line_range::{HighlightedLineRanges, LineRange, LineRanges},
    style::{StyleComponent, StyleComponents},
};
use console::Term;
use fancy_regex::Regex;
use mlua::prelude::*;
use once_cell::sync::Lazy;

use crate::{LmbError, LmbResult, Solution};

static LUA_ERROR_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\[[^\]]+\]:(\d+):.+")
        .expect("failed to compile regular expression for Lua error message")
});

/// Options to print script.
#[derive(Clone, Default)]
pub struct PrintOptions {
    /// Line number to be highlighted
    pub highlighted: Option<usize>,
    /// JSON mode
    pub json: bool,
    /// No colors <https://no-color.org/>
    pub no_color: bool,
    /// Theme
    pub theme: Option<String>,
}

impl PrintOptions {
    /// Create a option with "no color".
    pub fn no_color() -> Self {
        Self {
            no_color: true,
            ..Default::default()
        }
    }
}

/// Print script with syntax highlighting.
pub fn render_script<S, W>(mut f: W, script: S, options: &PrintOptions) -> anyhow::Result<bool>
where
    S: AsRef<str>,
    W: Write,
{
    let style_components = StyleComponents::new(&[StyleComponent::LineNumbers]);
    let mut config = Config {
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
    if let Some(line) = options.highlighted {
        let ranges = vec![LineRange::new(line, line)];
        config.highlighted_lines = HighlightedLineRanges(LineRanges::from(ranges));
    }
    let assets = HighlightingAssets::from_binary();
    let reader = Box::new(script.as_ref().as_bytes());
    let inputs = vec![bat::input::Input::from_reader(reader)];
    let controller = Controller::new(&config, &assets);
    Ok(controller.run(inputs, Some(&mut f))?)
}

fn render_lua_error<S, W>(
    mut f: W,
    script: S,
    message: S,
    options: &PrintOptions,
) -> anyhow::Result<()>
where
    S: AsRef<str>,
    W: Write,
{
    let first_line = message.as_ref().lines().next().unwrap_or_default();
    writeln!(f, "{first_line}")?;
    if let Ok(Some(c)) = LUA_ERROR_REGEX.captures(first_line) {
        let try_line_number = c.get(1).and_then(|n| n.as_str().parse::<usize>().ok());
        if let Some(line_number) = try_line_number {
            let mut new_options = options.clone();
            new_options.highlighted = Some(line_number);
            render_script(f, script, &new_options)?;
        }
    }
    Ok(())
}

/// Print solution when success or error and script when fail.
pub fn render_evaluation_result<R, S, W>(
    mut f: W,
    script: S,
    result: LmbResult<Solution<R>>,
    options: &PrintOptions,
) -> anyhow::Result<()>
where
    for<'lua> R: 'lua + Read,
    S: AsRef<str>,
    W: Write,
{
    match result {
        Ok(eval_result) => {
            let output = if options.json {
                serde_json::to_string(&eval_result.payload)?
            } else {
                eval_result.payload.to_string()
            };
            write!(f, "{output}")?;
            Ok(())
        }
        Err(e) => match &e {
            LmbError::Lua(LuaError::RuntimeError(message)) => {
                render_lua_error(f, script.as_ref(), message, options)?;
                Err(e.into())
            }
            LmbError::Lua(LuaError::SyntaxError { message, .. }) => {
                render_lua_error(f, script.as_ref(), message, options)?;
                Err(e.into())
            }
            _ => Err(e.into()),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::io::empty;

    use crate::{render_evaluation_result, render_script, EvaluationBuilder, PrintOptions};

    #[test]
    fn print_lua_code() {
        let mut buf = String::new();
        let options = PrintOptions::no_color();
        render_script(&mut buf, "return true", &options).unwrap();
        assert!(buf.contains("return true"));
    }

    #[test]
    fn print_solution() {
        let script = "return 1+1";
        let e = EvaluationBuilder::new(script, empty()).build();
        let result = e.evaluate();
        let mut buf = String::new();
        let options = PrintOptions::no_color();
        render_evaluation_result(&mut buf, script, result, &options).unwrap();
        assert_eq!("2", buf);
    }

    #[test]
    fn print_error() {
        let script = "return nil+1";
        let e = EvaluationBuilder::new(script, empty()).build();
        let result = e.evaluate();
        let mut buf = String::new();
        let options = PrintOptions::no_color();
        assert!(render_evaluation_result(&mut buf, script, result, &options).is_err());
        assert!(buf.contains("attempt to perform arithmetic (add) on nil and number"));
    }
}
