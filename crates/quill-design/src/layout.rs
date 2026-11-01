//! Layouts as data: which components a blog's pages are built from, and which
//! variant of each. Templates read this; content never depends on it.

use std::fmt;
use std::ops::Deref;

use serde::{Deserialize, Serialize};

const MAX_NAV_TAGS: u8 = 10;
const MAX_LABEL_CHARS: usize = 80;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageWidth {
    /// One reading column, as wide as the `measure` token.
    Narrow,
    Regular,
    Wide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeaderVariant {
    Centered,
    Left,
    Masthead,
}

/// The sidebar with the blog's description, tags and feed link.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TagRail {
    Left,
    Right,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedVariant {
    List,
    Grid,
    Cards,
    Compact,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArticleMeasure {
    Narrow,
    Wide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverStyle {
    Hero,
    Inline,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FooterVariant {
    Simple,
    Columns,
}

/// One block of the home page. The order of the list is the order on the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum HomeSection {
    /// The newest article as a large lead story.
    Featured,
    Feed {
        variant: FeedVariant,
    },
    ArchiveLink,
}

impl HomeSection {
    fn name(self) -> &'static str {
        match self {
            HomeSection::Featured => "featured",
            HomeSection::Feed { .. } => "feed",
            HomeSection::ArchiveLink => "archive_link",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    pub variant: HeaderVariant,
    /// How many of the most-used tags to list in the navigation.
    #[serde(default)]
    pub nav_tags: u8,
    #[serde(default)]
    pub nav_feed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArticleLayout {
    pub measure: ArticleMeasure,
    pub cover: CoverStyle,
    pub show_author: bool,
    pub show_date: bool,
    pub show_reading_time: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Footer {
    pub variant: FooterVariant,
    #[serde(default)]
    pub show_title: bool,
    #[serde(default)]
    pub show_feed: bool,
}

/// Wording that belongs to a layout, such as a terminal-styled blog calling its
/// home link `posts`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Labels {
    pub home: String,
    pub archive: String,
    pub feed: String,
    pub empty_home: String,
}

/// A layout that may not be valid yet. This is the editable form: fill it in,
/// then turn it into a [`Layout`] to validate it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutDraft {
    pub page_width: PageWidth,
    pub header: Header,
    pub tag_rail: TagRail,
    pub home: Vec<HomeSection>,
    pub article: ArticleLayout,
    pub footer: Footer,
    pub labels: Labels,
}

/// A validated layout. The only ways to get one go through validation, including
/// deserializing, so the rest of the program never has to re-check it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "LayoutDraft", into = "LayoutDraft")]
pub struct Layout(LayoutDraft);

impl Layout {
    pub fn from_toml(input: &str) -> Result<Self, LayoutError> {
        toml::from_str(input).map_err(|e| LayoutError(e.message().to_string()))
    }

    pub fn from_json(input: &str) -> Result<Self, LayoutError> {
        // Going through `Value` keeps "at line 1 column 57" out of the messages,
        // which mean nothing to someone using the dashboard.
        let value: serde_json::Value =
            serde_json::from_str(input).map_err(|e| LayoutError(e.to_string()))?;
        serde_json::from_value(value).map_err(|e| LayoutError(e.to_string()))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a layout always serializes")
    }

    /// An editable copy. Changes take effect only by validating it again.
    pub fn to_draft(&self) -> LayoutDraft {
        self.0.clone()
    }
}

/// Reading a layout's fields is always allowed; changing them isn't.
impl Deref for Layout {
    type Target = LayoutDraft;
    fn deref(&self) -> &LayoutDraft {
        &self.0
    }
}

impl TryFrom<LayoutDraft> for Layout {
    type Error = LayoutError;

    fn try_from(draft: LayoutDraft) -> Result<Self, LayoutError> {
        for (i, section) in draft.home.iter().enumerate() {
            if draft.home[..i].iter().any(|s| s.name() == section.name()) {
                return Err(LayoutError(format!(
                    "the home page can only have one `{}` section",
                    section.name()
                )));
            }
        }
        let has_feed = draft
            .home
            .iter()
            .any(|s| matches!(s, HomeSection::Feed { .. }));
        if !has_feed {
            return Err(LayoutError(
                "the home page needs a `feed` section to list articles".to_string(),
            ));
        }
        if draft.header.nav_tags > MAX_NAV_TAGS {
            return Err(LayoutError(format!(
                "the navigation can list at most {MAX_NAV_TAGS} tags, not {}",
                draft.header.nav_tags
            )));
        }
        let labels = [
            ("home", &draft.labels.home),
            ("archive", &draft.labels.archive),
            ("feed", &draft.labels.feed),
            ("empty_home", &draft.labels.empty_home),
        ];
        for (name, value) in labels {
            let chars = value.trim().chars().count();
            if chars == 0 || chars > MAX_LABEL_CHARS {
                return Err(LayoutError(format!(
                    "the `{name}` label must be 1 to {MAX_LABEL_CHARS} characters"
                )));
            }
        }
        Ok(Layout(draft))
    }
}

impl From<Layout> for LayoutDraft {
    fn from(layout: Layout) -> LayoutDraft {
        layout.0
    }
}

/// Why a layout was rejected, written for the person who has to fix it. Unknown
/// components and variants, missing parts and rule violations all end up here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutError(String);

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LayoutError {}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
        page_width = "regular"
        tag_rail = "right"

        [header]
        variant = "left"

        [[home]]
        type = "featured"

        [[home]]
        type = "feed"
        variant = "compact"

        [article]
        measure = "narrow"
        cover = "inline"
        show_author = true
        show_date = true
        show_reading_time = true

        [footer]
        variant = "simple"

        [labels]
        home = "Home"
        archive = "Archive"
        feed = "Feed"
        empty_home = "Nothing yet."
    "#;

    fn expect_error(input: &str) -> String {
        Layout::from_toml(input)
            .expect_err("should be rejected")
            .to_string()
    }

    #[test]
    fn parses_a_valid_layout_and_keeps_section_order() {
        let layout = Layout::from_toml(VALID).unwrap();
        assert_eq!(layout.header.variant, HeaderVariant::Left);
        assert_eq!(
            layout.home,
            [
                HomeSection::Featured,
                HomeSection::Feed {
                    variant: FeedVariant::Compact
                }
            ]
        );
    }

    #[test]
    fn round_trips_through_json_with_validation() {
        let layout = Layout::from_toml(VALID).unwrap();
        assert_eq!(Layout::from_json(&layout.to_json()).unwrap(), layout);
    }

    #[test]
    fn reports_unknown_variants_and_components() {
        let cases = [
            (VALID.replace("\"left\"", "\"zigzag\""), "zigzag"),
            (VALID.replace("\"compact\"", "\"mosaic\""), "mosaic"),
            (VALID.replace("\"featured\"", "\"carousel\""), "carousel"),
            (
                VALID.replace("[footer]", "[sidebar]\nx = 1\n[footer]"),
                "sidebar",
            ),
        ];
        for (input, name) in cases {
            let error = expect_error(&input);
            assert!(error.contains(name), "{error} should mention {name}");
        }
    }

    #[test]
    fn rejects_duplicate_sections() {
        let duplicate = VALID.replace(
            "[article]",
            "[[home]]\ntype = \"feed\"\nvariant = \"list\"\n[article]",
        );
        assert_eq!(
            expect_error(&duplicate),
            "the home page can only have one `feed` section"
        );
    }

    #[test]
    fn requires_a_feed_and_every_part() {
        let no_feed = VALID.replace(
            "type = \"feed\"\n        variant = \"compact\"",
            "type = \"archive_link\"",
        );
        assert_eq!(
            expect_error(&no_feed),
            "the home page needs a `feed` section to list articles"
        );
        let no_header = VALID.replace("[header]\n        variant = \"left\"", "");
        assert!(expect_error(&no_header).contains("header"));
        let no_date = VALID.replace("show_date = true", "");
        assert!(expect_error(&no_date).contains("show_date"));
    }

    #[test]
    fn rejects_out_of_range_values() {
        let tags = VALID.replace("variant = \"left\"", "variant = \"left\"\nnav_tags = 11");
        assert_eq!(
            expect_error(&tags),
            "the navigation can list at most 10 tags, not 11"
        );
        let blank = VALID.replace("feed = \"Feed\"", "feed = \"  \"");
        assert!(expect_error(&blank).contains("`feed` label"));
        let long = VALID.replace("feed = \"Feed\"", &format!("feed = \"{}\"", "x".repeat(81)));
        assert!(expect_error(&long).contains("`feed` label"));
    }

    #[test]
    fn stored_json_is_validated_too_with_a_clean_message() {
        let layout = Layout::from_toml(VALID).unwrap();
        let mut value = serde_json::to_value(&layout).unwrap();
        value["home"] = serde_json::json!([{"type": "archive_link"}]);
        assert_eq!(
            Layout::from_json(&value.to_string())
                .unwrap_err()
                .to_string(),
            "the home page needs a `feed` section to list articles"
        );
    }
}
