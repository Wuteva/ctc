use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// A compiled regular expression. Two patterns are equal when their text is.
#[derive(Clone, Debug)]
pub struct RegexPattern {
    source: String,
    regex: Arc<regex::Regex>,
}

impl RegexPattern {
    pub fn new(source: &str) -> Result<Self, String> {
        regex::Regex::new(source)
            .map(|regex| Self {
                source: source.to_string(),
                regex: Arc::new(regex),
            })
            .map_err(|error| error.to_string())
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// True when the pattern matches anywhere in `text`. Anchors such as `^`
    /// and `$` limit the match.
    pub fn is_match(&self, text: &str) -> bool {
        self.regex.is_match(text)
    }
}

impl PartialEq for RegexPattern {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
    }
}

impl Eq for RegexPattern {}

impl std::hash::Hash for RegexPattern {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.source.hash(state);
    }
}

impl Serialize for RegexPattern {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.source)
    }
}

impl<'de> Deserialize<'de> for RegexPattern {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let source = String::deserialize(deserializer)?;
        Self::new(&source).map_err(serde::de::Error::custom)
    }
}
