use std::collections::HashMap;
use std::fmt;

pub trait Summarize {
    fn summary(&self) -> String;
    fn word_count(&self) -> usize {
        self.summary().split_whitespace().count()
    }
}

#[derive(Debug, Clone)]
pub struct Article {
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
}

impl Article {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            tags: Vec::new(),
        }
    }

    pub fn add_tag(&mut self, tag: impl Into<String>) {
        self.tags.push(tag.into());
    }
}

impl Summarize for Article {
    fn summary(&self) -> String {
        format!("{}: {}", self.title, self.body.chars().take(80).collect::<String>())
    }
}

impl fmt::Display for Article {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.tags.join(", "), self.title)
    }
}

pub fn word_frequencies(text: &str) -> HashMap<String, usize> {
    let mut map = HashMap::new();
    for word in text.split_whitespace() {
        *map.entry(word.to_lowercase()).or_insert(0) += 1;
    }
    map
}

pub fn top_words(text: &str, n: usize) -> Vec<(String, usize)> {
    let mut pairs: Vec<_> = word_frequencies(text).into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    pairs.truncate(n);
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn article_summary_includes_title() {
        let a = Article::new("Hello", "World content here");
        assert!(a.summary().contains("Hello"));
    }

    #[test]
    fn word_count_via_trait() {
        let a = Article::new("Test", "one two three");
        assert!(a.word_count() > 0);
    }
}
