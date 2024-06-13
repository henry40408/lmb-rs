use ariadne::{CharSet, ColorGenerator, Config, Label, Report, ReportKind, Source};
use std::{
    fmt::Display,
    io::{Error as IoError, Write},
};

/// Container for the script used for syntax checking.
#[derive(Debug)]
pub struct LuaCheck {
    name: String,
    script: String,
}

impl LuaCheck {
    /// Create a new [`LuaCheck`] container.
    pub fn new<S>(name: S, script: S) -> Self
    where
        S: Display,
    {
        Self {
            name: name.to_string(),
            script: script.to_string(),
        }
    }

    /// Check the syntax of the script.
    ///
    /// # Errors
    ///
    /// This function will return an error if the script contains syntax errors.
    ///
    /// ```rust
    /// use lmb::LuaCheck;
    ///
    /// let check = LuaCheck::new("", "ret true");
    /// assert!(check.check().is_err());
    /// ```
    pub fn check(&self) -> Result<full_moon::ast::Ast, full_moon::Error> {
        full_moon::parse(self.script.as_ref())
    }

    /// Render an error from [`full_moon`] to a writer.
    ///
    /// # Errors
    ///
    /// This function will return an [`std::io::Error`] if there is an issue writing the error to the provided writer.
    pub fn write_error<W>(
        &self,
        mut f: W,
        err: full_moon::Error,
        no_color: bool,
    ) -> Result<(), IoError>
    where
        W: Write,
    {
        let mut colors = ColorGenerator::new();
        let color = colors.next();
        let name = &self.name;

        let (message, start, end) = match err {
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
            full_moon::Error::AstError(_) => return Ok(()),
            full_moon::Error::TokenizerError(e) => (
                e.error().to_string(),
                e.position().bytes(),
                e.position().bytes() + 1,
            ),
        };

        let span = start..end;
        Report::build(ReportKind::Error, name, start)
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
            .write((name, Source::from(&self.script)), &mut f)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::LuaCheck;

    #[test]
    fn syntax() {
        let script = "ret true";
        let check = LuaCheck::new("", script);
        assert!(matches!(
            check.check().unwrap_err(),
            full_moon::Error::AstError { .. }
        ));

        let script = "return true";
        let check = LuaCheck::new("", script);
        assert!(check.check().is_ok());
    }

    #[test]
    fn syntax_error() {
        let script = "ret true";
        let check = LuaCheck::new("", script);
        let err = check.check().unwrap_err();
        let mut buf = Vec::new();
        check.write_error(&mut buf, err, true).unwrap();
    }
}
