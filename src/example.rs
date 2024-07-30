use std::sync::LazyLock;

use full_moon::{tokenizer::TokenType, visitors::Visitor};
use include_dir::{include_dir, Dir};
use toml::{Table, Value};

/// Lua example.
#[derive(Debug, Default)]
pub struct Example {
    description: String,
    done: bool,
    name: String,
    script: String,
}

impl Example {
    /// Get description.
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Get name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get script.
    pub fn script(&self) -> &str {
        &self.script
    }
}

impl Visitor for Example {
    /// Extract the description from the first multi-line comment of a Lua script.
    fn visit_multi_line_comment(&mut self, token: &full_moon::tokenizer::Token) {
        if self.done {
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
        self.description = description.to_string();
        self.done = true;
    }
}

static EXAMPLES_DIR: Dir<'_> = include_dir!("lua-examples");

/// Embedded Lua examples.
pub static EXAMPLES: LazyLock<Vec<Example>> = LazyLock::new(|| {
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
            name: name.to_string(),
            script: script.to_string(),
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

#[cfg(test)]
mod tests {
    use crate::EXAMPLES;

    #[test]
    fn description_of_examples() {
        for e in EXAMPLES.iter() {
            let name = &e.name;
            assert!(!e.description.is_empty(), "{name} has no description");
        }
    }
}
