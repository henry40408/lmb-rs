use ariadne::{CharSet, ColorGenerator, Config, Label, Report, ReportKind, Source};
use bon::Builder;
use std::io::{Error as IoError, Write};

/// Container for the script used for syntax checking.
#[derive(Builder, Debug)]
pub struct LuaCheck {
    /// Name.
    #[builder(start_fn, into)]
    pub name: String,
    /// Script.
    #[builder(start_fn, into)]
    pub script: String,
}

impl LuaCheck {
    /// Check the syntax of the script.
    ///
    /// # Errors
    ///
    /// This function will return an error if the script contains syntax errors.
    ///
    /// ```rust
    /// use lmb::LuaCheck;
    ///
    /// let check = LuaCheck::builder("", "ret true").build();
    /// assert!(check.check().is_err());
    /// ```
    pub fn check(&self) -> Result<full_moon::ast::Ast, Vec<full_moon::Error>> {
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
        errors: Vec<full_moon::Error>,
        no_color: bool,
    ) -> Result<(), IoError>
    where
        W: Write,
    {
        let mut colors = ColorGenerator::new();
        let color = colors.next();
        let name = &self.name;

        let span = errors
            .iter()
            .min_by_key(|e| match e {
                full_moon::Error::AstError(e) => e.token().start_position().bytes(),
                full_moon::Error::TokenizerError(e) => e.position().bytes(),
            })
            .map(|e| match e {
                full_moon::Error::AstError(e) => {
                    let token = e.token();
                    token.start_position().bytes()..token.end_position().bytes()
                }
                full_moon::Error::TokenizerError(e) => e.position().bytes()..e.position().bytes(),
            });
        let mut report = Report::build(ReportKind::Error, (name, span.unwrap_or_else(|| 0..0)))
            .with_config(
                Config::default()
                    .with_char_set(CharSet::Ascii)
                    .with_compact(true)
                    .with_color(!no_color),
            );
        for error in errors {
            let (message, start, end) = match error {
                full_moon::Error::AstError(e) => (
                    e.error_message().to_string(),
                    e.token().start_position().bytes(),
                    e.token().end_position().bytes(),
                ),
                full_moon::Error::TokenizerError(e) => (
                    e.error().to_string(),
                    e.position().bytes(),
                    e.position().bytes() + 1,
                ),
            };
            let span = start..end;
            report = report
                .with_label(
                    Label::new((name, span))
                        .with_color(color)
                        .with_message(&message),
                )
                .with_message(&message);
        }
        report
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
        let check = LuaCheck::builder("", script).build();
        assert!(matches!(
            check.check().unwrap_err().get(0),
            Some(full_moon::Error::AstError { .. })
        ));

        let script = "return true";
        let check = LuaCheck::builder("", script).build();
        assert!(check.check().is_ok());
    }

    #[test]
    fn syntax_error() {
        let script = "ret true";
        let check = LuaCheck::builder("", script).build();
        let errors = check.check().unwrap_err();
        let mut buf = Vec::new();
        check.write_error(&mut buf, errors, true).unwrap();
    }
}
