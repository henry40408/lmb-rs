use ariadne::{CharSet, ColorGenerator, Config, Label, Report, ReportKind, Source};

/// Check syntax of Lua script.
///
/// ```rust
/// use lam::*;
///
/// let checked = check_syntax("ret true");
/// assert!(checked.is_err());
///
/// let checked = check_syntax("return true");
/// assert!(checked.is_ok());
/// ```
pub fn check_syntax<S: AsRef<str>>(script: S) -> Result<(), full_moon::Error> {
    full_moon::parse(script.as_ref())?;
    Ok(())
}

/// Return error message if syntax of Lua script has error.
///
/// ```rust
/// use lam::*;
///
/// let no_color = true;
/// let name = "test";
/// let script = "ret true";
/// let checked = check_syntax(script);
/// assert!(render_error(no_color, name, script, checked).is_some());
/// ```
pub fn render_error<S>(
    no_color: bool,
    name: S,
    script: S,
    result: Result<(), full_moon::Error>,
) -> Option<String>
where
    S: AsRef<str>,
{
    if let Err(full_moon::Error::AstError(full_moon::ast::AstError::UnexpectedToken {
        token,
        additional,
    })) = result
    {
        let mut colors = ColorGenerator::new();
        let color = colors.next();

        let mut buf = Vec::new();
        let name = name.as_ref();
        let message = additional.map_or(String::new(), |s| s.into_owned());
        let start = token.start_position().bytes();
        let end = token.end_position().bytes();
        let span = start..end;
        let _ = Report::build(ReportKind::Error, name, start)
            .with_config(
                Config::default()
                    .with_char_set(CharSet::Ascii)
                    .with_compact(true)
                    .with_color(!no_color),
            )
            .with_label(
                Label::new((name, span))
                    .with_color(color)
                    .with_message(&message),
            )
            .with_message(&message)
            .finish()
            .write((name, Source::from(script)), &mut buf);
        Some(String::from_utf8_lossy(&buf).to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use crate::{check_syntax, render_error};

    #[test]
    fn syntax() {
        let script = "ret true";
        assert!(matches!(
            check_syntax(script).unwrap_err(),
            full_moon::Error::AstError { .. }
        ));

        let script = "return true";
        assert!(check_syntax(script).is_ok());
    }

    #[test]
    fn syntax_error() {
        let name = "foobar";
        let script = "ret true";
        let res = check_syntax(script);
        assert!(render_error(true, name, script, res).is_some());
    }
}
