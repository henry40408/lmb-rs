use std::io::{self, Write};

use mlua::prelude::*;
use once_cell::sync::Lazy;
use regex::Regex;
use tabular::{Row, Table};

static MESSAGE_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?<name>.+?):(?<lineno>.+?):(?<message>.+)").expect("invalid regular expression")
});

pub fn check_syntax<S: AsRef<str>>(name: S, script: S) -> mlua::Result<()> {
    let vm = Lua::new();
    vm.load(script.as_ref())
        .set_name(name.as_ref().to_string())
        .into_function()?;
    Ok(())
}

pub fn parse_error_message<S: AsRef<str>>(message: S) -> Option<(usize, String)> {
    let message = message.as_ref().to_string();
    let caps = MESSAGE_PATTERN.captures(&message)?;
    let lineno = caps["lineno"].trim();
    let message = caps["message"].trim();
    Some((
        lineno
            .parse::<usize>()
            .expect("failed to convert line number"),
        message.to_string(),
    ))
}

pub fn print_error<S: AsRef<str>>(name: S, script: S, error: &mlua::Error) -> anyhow::Result<()> {
    let name = name.as_ref();
    if let mlua::Error::SyntaxError { message, .. } = error {
        let Some((lineno, message)) = parse_error_message(message) else {
            return Ok(());
        };

        let mut locked = io::stderr().lock();
        writeln!(locked, "syntax error: {message}")?;
        writeln!(locked, "--> {name}:{lineno}")?;

        let mut table = Table::new("{:>} | {:<}");
        for (index, line) in script.as_ref().lines().enumerate() {
            let current = index + 1;
            let line = if lineno == index + 1 {
                format!("{line} -- {message}")
            } else {
                line.to_string()
            };
            table.add_row(Row::new().with_cell(current).with_cell(line));
        }

        write!(locked, "{}", table)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{check_syntax, parse_error_message, print_error};

    #[test]
    fn syntax() {
        let script = "ret true";
        assert!(matches!(
            check_syntax("", script).unwrap_err(),
            mlua::Error::SyntaxError { .. }
        ));

        let script = "return true";
        assert!(check_syntax("", script).is_ok());
    }

    #[test]
    fn syntax_error() {
        let name = "foobar";
        let script = "ret true";
        let e = check_syntax("foobar", script).unwrap_err();
        assert!(matches!(e, mlua::Error::SyntaxError { .. }));
        if let mlua::Error::SyntaxError { message, .. } = &e {
            let (lineno, message) = parse_error_message(message).unwrap();
            assert_eq!(1, lineno);
            assert_eq!(
                "Incomplete statement: expected assignment or a function call",
                message
            );
        }
        print_error(name, script, &e).unwrap();
    }
}
