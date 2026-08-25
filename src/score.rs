//! Candidate scoring helpers — ports of readability's `_initializeNode`,
//! `_getClassWeight`, `_getLinkDensity`, `_getTextDensity` and `_getCharCount`.

use crate::dom::{self, Handle, NodeId};
use crate::regexes;
use std::cell::RefCell;
use std::collections::HashMap;

/// Per-node scores (the JS code stores these *on* the nodes; we keep a side
/// map keyed by node identity — cheaper than a parallel DOM).
#[derive(Default)]
pub struct Scores {
    map: RefCell<HashMap<NodeId, f64>>,
}

impl Scores {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, node: &Handle) -> f64 {
        self.map
            .borrow()
            .get(&dom::id(node))
            .copied()
            .unwrap_or(0.0)
    }

    pub fn set(&self, node: &Handle, v: f64) {
        self.map.borrow_mut().insert(dom::id(node), v);
    }

    pub fn add(&self, node: &Handle, v: f64) {
        *self.map.borrow_mut().entry(dom::id(node)).or_insert(0.0) += v;
    }

    pub fn has(&self, node: &Handle) -> bool {
        self.map.borrow().contains_key(&dom::id(node))
    }
}

/// Port of `_initializeNode`: base score from tag + class/id weight.
/// Returns the base content score. `weight_classes` mirrors the
/// `FLAG_WEIGHT_CLASSES` flag.
pub fn initialize_node(scores: &Scores, node: &Handle, weight_classes: bool) {
    let mut content_score = 0.0;
    if let Some(tag) = dom::tag_name(node) {
        match tag.as_str() {
            "DIV" => content_score += 5.0,
            "PRE" | "TD" | "BLOCKQUOTE" => content_score += 3.0,
            "ADDRESS" | "OL" | "UL" | "DL" | "DD" | "DT" | "LI" | "FORM" => content_score -= 3.0,
            "H1" | "H2" | "H3" | "H4" | "H5" | "H6" | "TH" => content_score -= 5.0,
            _ => {}
        }
    }
    if weight_classes {
        content_score += class_weight(node);
    }
    scores.set(node, content_score);
}

/// Port of `_getClassWeight`.
pub fn class_weight(node: &Handle) -> f64 {
    let mut weight = 0.0;
    let class = dom::class_name(node);
    if !class.is_empty() {
        if regexes::negative().is_match(&class) {
            weight -= 25.0;
        }
        if regexes::positive().is_match(&class) {
            weight += 25.0;
        }
    }
    if let Some(id) = dom::attr(node, "id") {
        if !id.is_empty() {
            if regexes::negative().is_match(&id) {
                weight -= 25.0;
            }
            if regexes::positive().is_match(&id) {
                weight += 25.0;
            }
        }
    }
    weight
}

/// Port of `_getLinkDensity`: fraction of text that lives inside anchors.
pub fn link_density(node: &Handle) -> f64 {
    let text_len = dom::inner_text(node, true).len();
    if text_len == 0 {
        return 0.0;
    }
    let mut link_len = 0.0;
    for a in dom::all_nodes_with_tag(node, &["A"]) {
        let href = dom::attr(&a, "href").unwrap_or_default();
        let coefficient = if regexes::hash_url().is_match(&href) {
            0.3
        } else {
            1.0
        };
        link_len += dom::inner_text(&a, true).len() as f64 * coefficient;
    }
    link_len / text_len as f64
}

/// Port of `_getTextDensity`: fraction of text inside the given tag set.
pub fn text_density(node: &Handle, tags: &[&str]) -> f64 {
    let text_len = dom::inner_text(node, true).len();
    if text_len == 0 {
        return 0.0;
    }
    let mut children_len = 0usize;
    for c in dom::all_nodes_with_tag(node, tags) {
        children_len += dom::inner_text(&c, true).len();
    }
    children_len as f64 / text_len as f64
}

/// Port of `_getCharCount(node, ",")` — commas (incl. CJK variants).
pub fn comma_count(node: &Handle) -> usize {
    regexes::commas()
        .find_iter(&dom::inner_text(node, true))
        .count()
}

/// Comma count over a plain string (mercury `scoreCommas(content)`).
pub fn comma_count_text(text: &str) -> usize {
    regexes::commas().find_iter(text).count()
}

/// Port of readability's `_textSimilarity` (1.0 = same text, 0.0 = fully
/// different): character-length ratio of deduplicated tokens over all
/// tokens (`REGEXPS.tokenize = /\W+/g`, `uniqTokensB = new Set(tokensB)`).
/// Port of readability's `_textSimilarity` (1.0 = same text, 0.0 = fully
/// different): `uniqTokensB = tokensB.filter(t => !tokensA.includes(t))`,
/// then `1 - (chars of missing B tokens / chars of all B tokens)`, space-
/// joined like the JS (`REGEXPS.tokenize = /\W+/g`).
pub fn text_similarity(text_a: &str, text_b: &str) -> f64 {
    let tokens_a: Vec<String> = regexes::non_word()
        .split(&text_a.to_lowercase())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect();
    let tokens_b: Vec<String> = regexes::non_word()
        .split(&text_b.to_lowercase())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect();
    if tokens_a.is_empty() || tokens_b.is_empty() {
        return 0.0;
    }
    // uniqTokensB = tokensB.filter(token => !tokensA.includes(token))
    let uniq_b: Vec<&String> = tokens_b.iter().filter(|t| !tokens_a.contains(t)).collect();
    let join_len = |tokens: &[&String]| -> usize {
        tokens.iter().map(|t| t.len()).sum::<usize>() + tokens.len().saturating_sub(1)
    };
    let total = join_len(&tokens_b.iter().collect::<Vec<_>>());
    if total == 0 {
        return 0.0;
    }
    let distance_b = join_len(&uniq_b) as f64 / total as f64;
    1.0 - distance_b
}
