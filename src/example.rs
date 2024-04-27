use std::borrow::Cow;

use full_moon::{tokenizer::TokenType, visitors::Visitor};
use include_dir::{include_dir, Dir};
use once_cell::sync::Lazy;
use toml::{Table, Value};

/// Lua example
#[derive(Default)]
pub struct Example<'a> {
    /// Name
    pub name: Cow<'a, str>,
    /// Description, which is extracted from the first multi-line comment as TOML
    pub description: Cow<'a, str>,
    /// Script
    pub script: Cow<'a, str>,
    _done: bool,
}

impl<'a> Visitor for Example<'a> {
    fn visit_multi_line_comment(&mut self, token: &full_moon::tokenizer::Token) {
        if self._done {
            return;
        }
        let TokenType::MultiLineComment { comment, .. } = token.token_type() else {
            return;
        };
        let comment = comment
            .split('\n')
            .map(|s| s.trim_start_matches('-'))
            .collect::<Vec<_>>()
            .join("\n");
        let Ok(parsed) = comment.trim_end_matches('-').to_string().parse::<Table>() else {
            return;
        };
        let Value::String(description) = &parsed["description"] else {
            return;
        };
        self.description = Cow::Owned(description.to_string());
        self._done = true;
    }
}

static EXAMPLES_DIR: Dir<'_> = include_dir!("lua-examples");

/// Embedded Lua examples
pub static EXAMPLES: Lazy<Vec<Example<'_>>> = Lazy::new(|| {
    let mut examples = vec![];
    for f in EXAMPLES_DIR
        .find("**/*.lua")
        .expect("failed to list Lua examples")
    {
        let Some(name) = f.path().file_stem().map(|f| f.to_string_lossy()) else {
            continue;
        };
        let Some(script) = f.as_file().and_then(|handle| handle.contents_utf8()) else {
            continue;
        };
        let mut example = Example {
            name,
            script: Cow::Owned(script.to_string()),
            ..Default::default()
        };
        let Ok(ast) = full_moon::parse(script) else {
            continue;
        };
        example.visit_ast(&ast);
        examples.push(example);
    }
    examples.sort_by(|a, b| a.name.cmp(&b.name));
    examples
});
