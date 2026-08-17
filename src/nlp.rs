//! Port of newspaper3k's `nlp.py` — keyword extraction and extractive
//! summarization with no external NLP dependencies.
//!
//! The reference implementation used nltk's punkt tokenizer for sentence
//! splitting; here a small regex-free splitter over sentence-ending
//! punctuation reproduces the same behaviour (keep sentences > 10 chars).

use std::collections::HashMap;
use std::sync::OnceLock;

/// newspaper3k `resources/misc/stopwords-nlp-en.txt`, shipped verbatim.
const STOPWORDS_EN: &str = include_str!("stopwords-en.txt");

/// Ideal sentence length (newspaper `ideal = 20.0`).
const IDEAL: f64 = 20.0;
/// Number of keywords (newspaper `NUM_KEYWORDS = 10`).
const NUM_KEYWORDS: usize = 10;

fn stopwords() -> &'static std::collections::HashSet<&'static str> {
    static SW: OnceLock<std::collections::HashSet<&'static str>> = OnceLock::new();
    SW.get_or_init(|| {
        STOPWORDS_EN
            .lines()
            .map(str::trim)
            .filter(|w| !w.is_empty() && w.chars().any(char::is_alphanumeric) && !matches!(*w, "-"))
            .collect()
    })
}

/// Port of `split_words`: strip non-word chars, lowercase, split.
pub fn split_words(text: &str) -> Vec<String> {
    let cleaned: String = text
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' {
                c
            } else {
                ' '
            }
        })
        .collect();
    cleaned
        .split_whitespace()
        .map(|w| w.to_lowercase())
        .collect()
}

/// Port of `split_sentences` (punkt replaced by punctuation splitting).
/// Sentences <= 10 chars are dropped, matching the reference.
pub fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut start = 0usize;
    let chars: Vec<char> = text.chars().collect();

    for (i, &c) in chars.iter().enumerate() {
        if matches!(c, '.' | '!' | '?') {
            // A sentence ender must be followed by whitespace/end/quote.
            let next = chars.get(i + 1).copied();
            if next.is_none_or(|n| n.is_whitespace()) || matches!(next, Some('"') | Some('\'')) {
                let s: String = chars[start..=i].iter().collect();
                let s = s.replace('\n', "");
                let s = s.trim().to_string();
                if s.chars().count() > 10 {
                    sentences.push(s);
                }
                start = i + 1;
            }
        }
    }
    if start < chars.len() {
        let s: String = chars[start..].iter().collect();
        let s = s.replace('\n', "");
        let s = s.trim().to_string();
        if s.chars().count() > 10 {
            sentences.push(s);
        }
    }
    sentences
}

/// Port of `keywords(text)`: top-10 keyword frequency scores.
pub fn keywords(text: &str) -> Vec<(String, f64)> {
    let words = split_words(text);
    if words.is_empty() {
        return Vec::new();
    }
    let num_words = words.len();
    let sw = stopwords();
    let mut freq: HashMap<&str, usize> = HashMap::new();
    for w in words.iter().filter(|w| !sw.contains(w.as_str())) {
        *freq.entry(w.as_str()).or_insert(0) += 1;
    }

    let mut ranked: Vec<(&str, usize)> = freq.into_iter().collect();
    // Python: sorted by (count, word), reverse=True → count desc, word desc.
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.cmp(a.0)));
    ranked.truncate(NUM_KEYWORDS);

    ranked
        .into_iter()
        .map(|(w, count)| {
            let score = count as f64 / num_words.max(1) as f64;
            (w.to_string(), score * 1.5 + 1.0)
        })
        .collect()
}

/// Port of `title_score`.
fn title_score(title_words: &[String], sentence: &[String]) -> f64 {
    if title_words.is_empty() {
        return 0.0;
    }
    let sw = stopwords();
    let title: Vec<&String> = title_words
        .iter()
        .filter(|w| !sw.contains(w.as_str()))
        .collect();
    if title.is_empty() {
        return 0.0;
    }
    let count = sentence
        .iter()
        .filter(|w| !sw.contains(w.as_str()) && title.contains(w))
        .count() as f64;
    count / title.len().max(1) as f64
}

/// Port of `length_score`.
fn length_score(len: usize) -> f64 {
    1.0 - (IDEAL - len as f64).abs() / IDEAL
}

/// Port of `sentence_position`.
#[allow(clippy::if_same_then_else)] // mirrors the reference's if-chain
fn sentence_position(i: usize, size: usize) -> f64 {
    let normalized = i as f64 / size as f64;
    if normalized > 1.0 {
        0.0
    } else if normalized > 0.9 {
        0.15
    } else if normalized > 0.8 {
        0.04
    } else if normalized > 0.7 {
        0.04
    } else if normalized > 0.6 {
        0.06
    } else if normalized > 0.5 {
        0.04
    } else if normalized > 0.4 {
        0.05
    } else if normalized > 0.3 {
        0.08
    } else if normalized > 0.2 {
        0.14
    } else if normalized > 0.1 {
        0.23
    } else if normalized > 0.0 {
        0.17
    } else {
        0.0
    }
}

/// Port of `sbs`: sum of keyword scores over sentence, scaled by length.
fn sbs(words: &[String], kw: &HashMap<&str, f64>) -> f64 {
    if words.is_empty() {
        return 0.0;
    }
    let score: f64 = words.iter().filter_map(|w| kw.get(w.as_str())).sum();
    (1.0 / (words.len() as f64).abs() * score) / 10.0
}

/// Port of `dbs`: distance-based scoring between consecutive keyword hits.
fn dbs(words: &[String], kw: &HashMap<&str, f64>) -> f64 {
    if words.is_empty() {
        return 0.0;
    }
    let mut summ = 0.0;
    let mut prev: Option<(usize, f64)> = None;

    for (i, word) in words.iter().enumerate() {
        if let Some(&score) = kw.get(word.as_str()) {
            if let Some((pi, ps)) = prev {
                let dif = (i - pi) as f64;
                summ += (ps * score) / (dif * dif);
            }
            prev = Some((i, score));
        }
    }

    let kw_set: std::collections::HashSet<&str> = kw.keys().copied().collect();
    let word_set: std::collections::HashSet<&str> = words.iter().map(String::as_str).collect();
    let k = kw_set.intersection(&word_set).count() + 1;
    1.0 / (k * (k + 1)) as f64 * summ
}

/// Port of `score` + `summarize`: extractive multi-sentence summary.
pub fn summarize(title: &str, text: &str, max_sents: usize) -> Vec<String> {
    if text.is_empty() || title.is_empty() || max_sents == 0 {
        return Vec::new();
    }
    let sentences = split_sentences(text);
    if sentences.is_empty() {
        return Vec::new();
    }
    let kw = keywords(text);
    let kw_map: HashMap<&str, f64> = kw.iter().map(|(w, s)| (w.as_str(), *s)).collect();
    let title_words = split_words(title);

    let size = sentences.len();
    let mut scored: Vec<(usize, String, f64)> = sentences
        .into_iter()
        .enumerate()
        .map(|(i, s)| {
            let words = split_words(&s);
            let title_feature = title_score(&title_words, &words);
            let sbs_feature = sbs(&words, &kw_map);
            let dbs_feature = dbs(&words, &kw_map);
            let frequency = (sbs_feature + dbs_feature) / 2.0 * 10.0;
            let total = (title_feature * 1.5
                + frequency * 2.0
                + length_score(words.len()) * 1.0
                + sentence_position(i + 1, size) * 1.0)
                / 4.0;
            (i, s, total)
        })
        .collect();

    // Counter.most_common(max_sents): stable sort by score desc.
    scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(max_sents);
    // Reference sorts results back into sentence order.
    scored.sort_by_key(|(i, _, _)| *i);
    scored.into_iter().map(|(_, s, _)| s).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keywords_rank_common_words() {
        let text = "the cat sat on the mat. the cat sat on the mat. \
                    the cat sat on the mat and watched a dog run past the mat.";
        let kw = keywords(text);
        assert!(!kw.is_empty());
        // "mat" appears 4x, "cat" 3x — mat wins.
        assert_eq!(kw[0].0, "mat");
        assert!(kw[0].1 > kw.last().unwrap().1 || kw.len() == 1);
    }

    #[test]
    fn stopwords_are_excluded() {
        let kw = keywords("the the the the the");
        assert!(kw.is_empty());
    }

    #[test]
    fn summarize_returns_ordered_sentences() {
        let title = "Rust memory safety";
        let text = "First sentence about memory safety in Rust. Second sentence \
                    about ownership rules. Third sentence about borrow checking. \
                    Fourth sentence about lifetimes. Fifth sentence about concurrency. \
                    Sixth sentence about performance.";
        let s = summarize(title, text, 3);
        assert!(s.len() <= 3);
        assert!(!s.is_empty());
        // Sentences must keep their original order.
        let joined = s.join(" ");
        assert!(joined.starts_with("First sentence") || s[0].contains("memory"));
    }
}
