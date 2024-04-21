use annotate_snippets::{Level, Snippet};

pub fn check_syntax<S: AsRef<str>>(script: S) -> Result<(), full_moon::Error> {
    full_moon::parse(script.as_ref())?;
    Ok(())
}

pub fn render_error<'a, S>(
    name: &'a S,
    script: &'a S,
    error: &'a full_moon::Error,
) -> Option<annotate_snippets::Message<'a>>
where
    S: AsRef<str> + ?Sized + 'a,
{
    if let full_moon::Error::AstError(full_moon::ast::AstError::UnexpectedToken {
        token,
        additional,
    }) = error
    {
        let message = additional.as_deref().unwrap_or("");
        let span = token.start_position().bytes()..token.end_position().bytes();
        let message = Level::Error.title(message).snippet(
            Snippet::source(script.as_ref())
                .origin(name.as_ref())
                .annotation(Level::Error.span(span).label(message)),
        );
        Some(message)
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
        let e = check_syntax(script).unwrap_err();
        assert!(matches!(e, full_moon::Error::AstError { .. }));

        render_error(name, script, &e).unwrap();
    }
}
