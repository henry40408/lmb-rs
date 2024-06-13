use include_dir::{include_dir, Dir};
use once_cell::sync::Lazy;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Guide.
pub struct Guide {
    content: String,
    name: String,
    title: String,
}

impl Guide {
    /// Get name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Title.
    pub fn title(&self) -> &str {
        &self.title
    }
}

static GUIDE_DIR: Dir<'_> = include_dir!("guides");

/// Guides.
pub static GUIDES: Lazy<Vec<Guide>> = Lazy::new(|| {
    let mut guides = vec![];
    for f in GUIDE_DIR.find("**/*.md").expect("failed to list guides") {
        let Some(name) = f.path().file_stem().map(|f| f.to_string_lossy()) else {
            continue;
        };
        let Some(content) = f.as_file().and_then(|handle| handle.contents_utf8()) else {
            continue;
        };
        let mut title = String::new();
        let parser = Parser::new_ext(content, Options::all());
        let mut is_heading = false;
        for event in parser {
            match event {
                Event::Start(Tag::Heading { level, .. }) => {
                    if !is_heading && level == HeadingLevel::H1 {
                        is_heading = true;
                    }
                }
                Event::End(TagEnd::Heading(_)) => {
                    if is_heading {
                        is_heading = false;
                    }
                }
                Event::Text(s) => {
                    if is_heading {
                        title.push_str(&s);
                    }
                }
                _ => {}
            }
        }
        guides.push(Guide {
            content: content.to_string(),
            name: name.to_string(),
            title,
        });
    }
    guides.sort_by(|a, b| a.title.cmp(&b.title));
    guides
});

#[cfg(test)]
mod tests {
    use crate::guide::GUIDES;

    #[test]
    fn guides() {
        assert!(!GUIDES.is_empty());

        let guide = GUIDES.first().unwrap();
        assert_eq!("lua", guide.name());
        assert_eq!("Lua Guide with Lmb", guide.title());
    }
}
