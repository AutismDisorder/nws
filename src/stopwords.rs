//! Port of newspaper3k's `text.py` — `StopWords` + `WordStats` and the
//! stopword-counting machinery used by the newspaper scoring algorithms.

use std::collections::HashSet;
use std::sync::OnceLock;

/// newspaper3k `resources/text/stopwords-en.txt`, shipped verbatim.
const STOPWORDS_EN: &str = include_str!("stopwords-en.txt");

/// Port of `WordStats`.
#[derive(Debug, Default, Clone)]
pub struct WordStats {
    pub stop_word_count: usize,
    pub word_count: usize,
    pub stop_words: Vec<String>,
}

/// Port of `StopWords` (English stopword set, cached).
pub fn stopwords_en() -> &'static HashSet<&'static str> {
    static SW: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SW.get_or_init(|| {
        STOPWORDS_EN
            .lines()
            .map(str::trim)
            .filter(|w| !w.is_empty())
            .collect()
    })
}

/// Port of `remove_punctuation`: strip ASCII punctuation from a string.
fn remove_punctuation(content: &str) -> String {
    content
        .chars()
        .filter(|c| !c.is_ascii_punctuation())
        .collect()
}

/// Port of `StopWords.get_stopword_count(content)`.
pub fn get_stopword_count(content: &str) -> WordStats {
    let mut ws = WordStats::default();
    if content.is_empty() {
        return ws;
    }
    let stripped = remove_punctuation(content);
    let stop = stopwords_en();
    let mut words = Vec::new();
    for w in stripped.split_whitespace().map(|w| w.to_lowercase()) {
        ws.word_count += 1;
        if stop.contains(w.as_str()) {
            ws.stop_word_count += 1;
            words.push(w);
        }
    }
    ws.stop_words = words;
    ws
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_stopwords() {
        let ws = get_stopword_count("The quick brown fox jumps over the lazy dog");
        // "the" x2 + "over" — per newspaper's stopwords-en.txt.
        assert_eq!(ws.stop_word_count, 3);
        assert_eq!(ws.word_count, 9);
        assert!(ws.stop_words.iter().all(|w| w == "the" || w == "over"));
    }

    #[test]
    fn empty_input() {
        let ws = get_stopword_count("");
        assert_eq!(ws.word_count, 0);
    }
}
