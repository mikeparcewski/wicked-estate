use std::collections::HashMap;

/// Counts the frequency of each word in the given text.
pub fn word_frequencies(text: &str) -> HashMap<String, usize> {
    let mut map = HashMap::new();
    for word in text.split_whitespace() {
        let key = word.to_lowercase();
        *map.entry(key).or_insert(0) += 1;
    }
    map
}

/// Returns the top-N words by frequency, sorted descending.
pub fn top_words(text: &str, n: usize) -> Vec<(String, usize)> {
    let freq = word_frequencies(text);
    let mut pairs: Vec<(String, usize)> = freq.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    pairs.truncate(n);
    pairs
}

/// Formats a frequency report as a human-readable string.
pub fn format_report(text: &str, n: usize) -> String {
    let words = top_words(text, n);
    let mut out = String::from("Word Frequency Report\n");
    for (word, count) in words {
        out.push_str(&format!("  {:20} {}\n", word, count));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_word_frequencies() {
        let freq = word_frequencies("hello world hello");
        assert_eq!(freq["hello"], 2);
        assert_eq!(freq["world"], 1);
    }

    #[test]
    fn test_top_words() {
        let top = top_words("a b a c a b", 2);
        assert_eq!(top[0], ("a".to_string(), 3));
        assert_eq!(top[1], ("b".to_string(), 2));
    }
}
