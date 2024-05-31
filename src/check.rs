use ariadne::{CharSet, ColorGenerator, Config, Label, Report, ReportKind, Source};

/// Check syntax of Lua script.
///
/// ```rust
/// use lmb::*;
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
/// use lmb::*;
///
/// let no_color = true;
/// let name = "test";
/// let script = "ret true";
/// let checked = check_syntax(script);
/// assert!(render_fullmoon_result(no_color, name, script, &checked.unwrap_err()).is_some());
/// ```
pub fn render_fullmoon_result<S>(
    no_color: bool,
    name: S,
    script: S,
    result: &full_moon::Error,
) -> Option<String>
where
    S: AsRef<str>,
{
    let mut colors = ColorGenerator::new();
    let color = colors.next();
    let name = name.as_ref();

    let (message, start, end) = match result {
        full_moon::Error::TokenizerError(e) => (
            e.error().to_string(),
            e.position().bytes(),
            e.position().bytes() + 1,
        ),
        full_moon::Error::AstError(full_moon::ast::AstError::UnexpectedToken {
            token,
            additional,
        }) => (
            additional
                .as_ref()
                .map_or_else(String::new, |s| s.to_string()),
            token.start_position().bytes(),
            token.end_position().bytes(),
        ),
        full_moon::Error::AstError(_) => return None,
    };

    let mut buf = Vec::new();
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

    Some(String::from_utf8_lossy(&buf).trim().to_string())
}

#[cfg(test)]
mod tests {
    use crate::{check_syntax, render_fullmoon_result};

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
        assert!(render_fullmoon_result(true, name, script, &res.unwrap_err()).is_some());
    }
}
