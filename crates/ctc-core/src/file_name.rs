use std::path::Path;

use serde::{Deserialize, Serialize};

/// The case used to turn a file stem into an identifier.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum NameCase {
    #[serde(rename = "PascalCase")]
    Pascal,
    #[serde(rename = "camelCase")]
    Camel,
    #[serde(rename = "snake_case")]
    Snake,
    #[serde(rename = "asIs")]
    AsIs,
}

impl NameCase {
    pub const NAMES: [&'static str; 4] = ["PascalCase", "camelCase", "snake_case", "asIs"];

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "PascalCase" => Some(Self::Pascal),
            "camelCase" => Some(Self::Camel),
            "snake_case" => Some(Self::Snake),
            "asIs" => Some(Self::AsIs),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Pascal => "PascalCase",
            Self::Camel => "camelCase",
            Self::Snake => "snake_case",
            Self::AsIs => "asIs",
        }
    }

    pub fn convert(self, stem: &str) -> String {
        match self {
            Self::Pascal => split_words(stem)
                .iter()
                .map(|word| capitalize(word))
                .collect(),
            Self::Camel => split_words(stem)
                .iter()
                .enumerate()
                .map(|(index, word)| match index {
                    0 => word.to_lowercase(),
                    _ => capitalize(word),
                })
                .collect(),
            Self::Snake => split_words(stem)
                .iter()
                .map(|word| word.to_lowercase())
                .collect::<Vec<_>>()
                .join("_"),
            Self::AsIs => stem.to_string(),
        }
    }
}

/// Returns the file name of a path and its stem. The stem drops only the last
/// extension, so `user.service.ts` gives `user.service`.
pub fn file_name_and_stem(path: &str) -> (&str, &str) {
    let path = Path::new(path);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("");
    (name, stem)
}

/// Splits text into words. Any character that is not a letter or digit ends a
/// word. A word also ends before an uppercase letter that follows a lowercase
/// letter or digit, and before the last uppercase letter of an uppercase run
/// when a lowercase letter follows it. Digits stay with the word before them.
pub fn split_words(text: &str) -> Vec<String> {
    let characters = text.chars().collect::<Vec<_>>();
    let mut words = Vec::new();
    let mut current = String::new();
    for (index, &character) in characters.iter().enumerate() {
        if !character.is_alphanumeric() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            continue;
        }
        if character.is_uppercase() && !current.is_empty() {
            let previous = characters[index - 1];
            let next_is_lower = characters
                .get(index + 1)
                .is_some_and(|next| next.is_lowercase());
            if previous.is_lowercase()
                || previous.is_numeric()
                || (previous.is_uppercase() && next_is_lower)
            {
                words.push(std::mem::take(&mut current));
            }
        }
        current.push(character);
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn capitalize(word: &str) -> String {
    let mut characters = word.chars();
    match characters.next() {
        Some(first) => first
            .to_uppercase()
            .chain(characters.flat_map(char::to_lowercase))
            .collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests;
