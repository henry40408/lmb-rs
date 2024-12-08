use std::sync::LazyLock;

use bon::Builder;
use include_dir::{include_dir, Dir};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Guide.
#[derive(Builder, Debug)]
pub struct Guide {
    /// Content.
    #[builder(into)]
    pub content: String,
    /// Name.
    #[builder(into)]
    pub name: String,
    /// Title.
    pub title: String,
}

static GUIDE_DIR: Dir<'_> = include_dir!("guides");

/// Guides.
pub static GUIDES: LazyLock<Vec<Guide>> = LazyLock::new(|| {
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
        guides.push(
            Guide::builder()
                .content(content)
                .name(name)
                .title(title)
                .build(),
        );
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
        assert_eq!("lua", guide.name);
        assert_eq!("Lua Guide with Lmb", guide.title);
    }
}
