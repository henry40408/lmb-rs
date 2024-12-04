use std::{fmt::Write, io::Read};

use ariadne::{CharSet, ColorGenerator, Label, Report, ReportKind, Source, Span};
use lazy_regex::{lazy_regex, Lazy, Regex};
use mlua::prelude::*;
use thiserror::Error;

use crate::{Evaluation, Result};

static LUA_ERROR_REGEX: Lazy<Regex> = lazy_regex!(r"\[[^\]]+\]:(\d+):(.+)");

/// Custom error type for handling various error scenarios.
#[derive(Debug, Error)]
pub enum Error {
    /// Error from the [`bat`] library
    #[error("bat error: {0}")]
    Bat(#[from] bat::error::Error),
    /// Error from the `SQLite` database
    #[error("sqlite error: {0}")]
    Database(#[from] rusqlite::Error),
    /// Error from database migration
    #[error("migration error: {0}")]
    DatabaseMigration(#[from] rusqlite_migration::Error),
    /// Error in formatting output
    #[error("format error: {0}")]
    Format(#[from] std::fmt::Error),
    /// Invalid key length for HMAC
    #[error("invalid length: {0}")]
    InvalidLength(#[from] crypto_common::InvalidLength),
    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// Error from the Lua engine
    #[error("lua error: {0}")]
    Lua(#[from] LuaError),
    /// Error decoding value from `MessagePack` format
    #[error("RMP decode error: {0}")]
    RMPDecode(#[from] rmp_serde::decode::Error),
    /// Error encoding value to `MessagePack` format
    #[error("RMP encode error: {0}")]
    RMPEncode(#[from] rmp_serde::encode::Error),
    /// Error from [`serde_json`] library
    #[error("serde JSON error: {0}")]
    SerdeJSONError(#[from] serde_json::Error),
}

impl Error {
    /// Render a Lua runtime or syntax error.
    pub fn write_lua_error<R, W>(&self, mut f: W, e: &Evaluation<R>, no_color: bool) -> Result<()>
    where
        for<'lua> R: 'lua + Read + Send,
        W: Write,
    {
        let message = match self {
            Self::Lua(LuaError::RuntimeError(message) | LuaError::SyntaxError { message, .. }) => {
                message
            }
            _ => return Ok(()),
        };

        let first_line = message.lines().next().unwrap_or_default();
        let Some(captures) = LUA_ERROR_REGEX.captures(first_line) else {
            return Ok(write!(f, "{}", first_line)?);
        };

        let Some(line_number) = captures
            .get(1)
            .and_then(|n| n.as_str().parse::<usize>().ok())
        else {
            return Ok(write!(f, "{}", first_line)?);
        };

        let mut colors = ColorGenerator::new();

        let source = Source::from(e.script());
        let line = source
            .line(line_number - 1) // index, not line number
            .expect("cannot find line in source");
        let span = line.span();

        let message = captures.get(2).map_or(first_line, |s| s.as_str().trim());
        let mut buf = Vec::new();
        Report::build(ReportKind::Error, (e.name(), span.start()..span.end()))
            .with_config(
                ariadne::Config::default()
                    .with_char_set(CharSet::Ascii)
                    .with_compact(true)
                    .with_color(!no_color),
            )
            .with_label(
                Label::new((e.name(), span))
                    .with_color(colors.next())
                    .with_message(message),
            )
            .with_message(message)
            .finish()
            .write((e.name(), source), &mut buf)?;
        write!(f, "{}", String::from_utf8_lossy(&buf))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::empty;

    use crate::build_evaluation;

    #[test]
    fn write_error() {
        let script = "return nil+1";
        let e = build_evaluation(script, empty()).call().unwrap();
        let Err(err) = e.evaluate() else {
            panic!("expect error");
        };
        let mut buf = String::new();
        err.write_lua_error(&mut buf, &e, true).unwrap();
        assert!(buf.contains("attempt to perform arithmetic (add) on nil and number"));
    }
}
