//! Port of newspaper3k's `ContentExtractor.calculate_best_node` and friends —
//! newspaper's *own* body-extraction algorithm (stopword-count scoring,
//! first-paragraph boosting, negative scoring at the tail, sibling walking).
//! Used as a fallback extractor when readability's grab falls short.

use crate::dom::{self, Handle};
use crate::stopwords;

/// Port of `nodes_to_check`: paragraphs, preformatted blocks, table cells
/// — skipping nodes hidden by their own or an ancestor's inline style
/// (readability removes those before scoring; the fallback must not
/// resurrect them).
fn nodes_to_check(doc: &Handle) -> Vec<Handle> {
    dom::all_nodes_with_tag(doc, &["P", "PRE", "TD"])
        .into_iter()
        .filter(visible_with_ancestors)
        .collect()
}

fn visible_with_ancestors(node: &Handle) -> bool {
    let mut cur = Some(node.clone());
    let mut depth = 0;
    while let Some(n) = cur {
        if !dom::is_probably_visible(&n) {
            return false;
        }
        if depth > 3 {
            break;
        }
        cur = dom::parent(&n);
        depth += 1;
    }
    true
}

/// Port of `is_highlink_density`.
fn is_highlink_density(e: &Handle) -> bool {
    let links = dom::all_nodes_with_tag(e, &["A"]);
    if links.is_empty() {
        return false;
    }
    let text = dom::inner_text(e, true);
    let words_number = text
        .split_whitespace()
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .count() as f64;
    if words_number == 0.0 {
        return true;
    }
    let link_text: String = links
        .iter()
        .map(|l| dom::inner_text(l, true))
        .collect::<Vec<_>>()
        .join("");
    let link_words = link_text.split_whitespace().count() as f64;
    let num_links = links.len() as f64;
    let link_divisor = link_words / words_number;
    let score = link_divisor * num_links;
    score >= 1.0
}

/// Port of `update_score` (the `gravityScore` attribute).
fn update_score(node: &Handle, add_to_score: f64) {
    let current = dom::attr(node, "gravityScore")
        .as_deref()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);
    dom::set_attr(node, "gravityScore", &format!("{}", current + add_to_score));
}

/// Port of `update_node_count` (the `gravityNodes` attribute).
fn update_node_count(node: &Handle, add_to_count: i64) {
    let current = dom::attr(node, "gravityNodes")
        .as_deref()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    dom::set_attr(node, "gravityNodes", &format!("{}", current + add_to_count));
}

fn get_score(node: &Handle) -> f64 {
    dom::attr(node, "gravityScore")
        .as_deref()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0)
}

/// Port of `walk_siblings` (previous siblings).
fn walk_siblings(node: &Handle) -> Vec<Handle> {
    let mut out = Vec::new();
    let mut sib = dom::previous_element_sibling(node);
    while let Some(s) = sib {
        out.push(s.clone());
        sib = dom::previous_element_sibling(&s);
    }
    out
}

/// Port of `is_boostable`.
fn is_boostable(node: &Handle) -> bool {
    let max_steps_away = 3usize;
    let minimum_stopword_count = 5usize;
    let mut steps_away = 0usize;

    for current in walk_siblings(node) {
        if dom::tag_is(&current, "P") {
            if steps_away >= max_steps_away {
                return false;
            }
            let ws = stopwords::get_stopword_count(&dom::inner_text(&current, true));
            if ws.stop_word_count > minimum_stopword_count {
                return true;
            }
            steps_away += 1;
        }
    }
    false
}

/// Port of `calculate_best_node`: score candidate paragraphs by stopword
/// count, boost early paragraphs, propagate scores to parents, pick the
/// highest-scoring container.
pub fn calculate_best_node(doc: &Handle) -> Option<Handle> {
    let mut nodes_with_text = Vec::new();
    for node in nodes_to_check(doc) {
        let text = dom::inner_text(&node, true);
        let ws = stopwords::get_stopword_count(&text);
        if ws.stop_word_count > 2 && !is_highlink_density(&node) {
            nodes_with_text.push(node);
        }
    }

    let nodes_number = nodes_with_text.len();
    if nodes_number == 0 {
        return None;
    }
    let bottom_negative = nodes_number as f64 * 0.25;

    let mut starting_boost = 1.0;
    let negative_scoring = 0.0;
    let mut parent_nodes: Vec<Handle> = Vec::new();

    for (i, node) in nodes_with_text.iter().enumerate() {
        let mut boost_score = 0.0;
        if is_boostable(node) {
            // The reference guards with `cnt >= 0`, which is always true.
            boost_score = (1.0 / starting_boost) * 50.0;
            starting_boost += 1.0;
        }
        if nodes_number > 15 && (nodes_number - i) as f64 <= bottom_negative {
            let booster = bottom_negative - (nodes_number - i) as f64;
            boost_score = -booster.powi(2);
            let negscore = boost_score.abs() + negative_scoring;
            if negscore > 40.0 {
                boost_score = 5.0;
            }
        }

        let ws = stopwords::get_stopword_count(&dom::inner_text(node, true));
        let upscore = ws.stop_word_count as f64 + boost_score;

        if let Some(parent) = dom::parent(node) {
            update_score(&parent, upscore);
            update_node_count(&parent, 1);
            if !parent_nodes.iter().any(|p| dom::id(p) == dom::id(&parent)) {
                parent_nodes.push(parent.clone());
            }

            if let Some(grand) = dom::parent(&parent) {
                update_node_count(&grand, 1);
                update_score(&grand, upscore / 2.0);
                if !parent_nodes.iter().any(|p| dom::id(p) == dom::id(&grand)) {
                    parent_nodes.push(grand);
                }
            }
        }
    }

    let mut top: Option<Handle> = None;
    let mut top_score = 0.0f64;
    for e in parent_nodes {
        let score = get_score(&e);
        if top.is_none() || score > top_score {
            top = Some(e);
            top_score = score;
        }
    }
    top
}

/// Port of `get_siblings_score`: average stopword score of *visible*
/// paragraphs inside the top node (base line for sibling absorption).
fn get_siblings_score(top_node: &Handle) -> f64 {
    let mut paragraphs_number = 0usize;
    let mut paragraphs_score = 0usize;
    for node in dom::all_nodes_with_tag(top_node, &["P"])
        .into_iter()
        .filter(visible_with_ancestors)
    {
        let ws = stopwords::get_stopword_count(&dom::inner_text(&node, true));
        if ws.stop_word_count > 2 && !is_highlink_density(&node) {
            paragraphs_number += 1;
            paragraphs_score += ws.stop_word_count;
        }
    }
    if paragraphs_number > 0 {
        paragraphs_score as f64 / paragraphs_number as f64
    } else {
        100_000.0
    }
}

/// Port of `get_siblings_content`.
fn get_siblings_content(current_sibling: &Handle, baseline: f64) -> Vec<Handle> {
    if dom::tag_is(current_sibling, "P") && !dom::inner_text(current_sibling, true).is_empty() {
        return vec![current_sibling.clone()];
    }
    let mut ps = Vec::new();
    for first_paragraph in dom::all_nodes_with_tag(current_sibling, &["P"]) {
        let text = dom::inner_text(&first_paragraph, true);
        if !text.is_empty() {
            let ws = stopwords::get_stopword_count(&text);
            let paragraph_score = ws.stop_word_count as f64;
            let sibling_baseline = 0.30;
            let score = baseline * sibling_baseline;
            if score < paragraph_score && !is_highlink_density(&first_paragraph) {
                let p = dom::create_element("p");
                dom::append_child(&p, &dom::create_text(&text));
                ps.push(p);
            }
        }
    }
    ps
}

/// Port of `add_siblings`: absorb preceding *visible* siblings that score
/// above the baseline into the top node.
fn add_siblings(top_node: &Handle) {
    let baseline = get_siblings_score(top_node);
    let siblings = walk_siblings(top_node);
    for sib in siblings {
        if !visible_with_ancestors(&sib) {
            continue;
        }
        let ps = get_siblings_content(&sib, baseline);
        for p in ps {
            let first = dom::child_nodes(top_node).first().cloned();
            if let Some(ref_node) = first {
                dom::insert_before(top_node, &p, &ref_node);
            } else {
                dom::append_child(top_node, &p);
            }
        }
    }
}

/// Port of `post_cleanup`: add siblings, drop high-link-density non-p nodes.
pub fn post_cleanup(top_node: &Handle) {
    add_siblings(top_node);
    let children = dom::children(top_node);
    for e in children {
        if !dom::tag_is(&e, "P") && is_highlink_density(&e) {
            dom::detach(&e);
        }
    }
}

/// Port of `is_nodescore_threshold_met` — defined but never called in the
/// reference either; kept for fidelity.
#[allow(dead_code)]
fn is_nodescore_threshold_met(node: &Handle, e: &Handle) -> bool {
    let top_score = get_score(node);
    let current_score = get_score(e);
    let threshold = top_score * 0.08;
    (current_score >= threshold) || dom::tag_is(e, "TD")
}

/// Port of `is_table_and_no_para_exist` — unused in the reference too.
#[allow(dead_code)]
fn is_table_and_no_para_exist(e: &Handle) -> bool {
    for p in dom::all_nodes_with_tag(e, &["P"]) {
        if dom::inner_text(&p, true).len() < 25 {
            dom::detach(&p);
        }
    }
    dom::all_nodes_with_tag(e, &["P"]).is_empty() && !dom::tag_is(e, "TD")
}

/// Full newspaper fallback extraction: best node + `post_cleanup`
/// (sibling absorption + high-link-density pruning), exactly as
/// `article.py` drives it.
pub fn newspaper_extract(doc: &Handle) -> Option<Handle> {
    let top = calculate_best_node(doc)?;
    post_cleanup(&top);
    Some(top)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"
    <html><body>
    <div class="article">
    <h1>Title here</h1>
    <p>This paragraph contains the word the several times over, and the
    algorithm counts those stopwords to score it, and the more the better
    for the purposes of the test, so here are the and the again.</p>
    <p>Another paragraph with the and the and the repeated, and and and
    the the the over and over, so the counter has a lot of material here.</p>
    <p>Third paragraph with the the the the the the the the the the the
    and and and and and and and and and and of of of of of of of of of.</p>
    </div>
    <div class="footer"><a href="/1">link</a><a href="/2">link</a><a href="/3">link</a><a href="/4">link</a></div>
    </body></html>"#;

    #[test]
    fn picks_the_article_container() {
        let dom = dom::parse(PAGE);
        let doc = dom::document(&dom);
        let top = newspaper_extract(&doc).expect("best node");
        let text = dom::inner_text(&top, true);
        assert!(text.contains("Title here"));
        assert!(!text.contains("link"));
    }

    #[test]
    fn high_link_density_detected() {
        let dom = dom::parse("<div><a href='/1'>one link here</a> <a href='/2'>two</a> <a href='/3'>three</a> <a href='/4'>four</a></div>");
        let doc = dom::document(&dom);
        let div = dom::all_nodes_with_tag(&doc, &["DIV"])[0].clone();
        assert!(is_highlink_density(&div));
    }

    #[test]
    fn stopword_scoring_boosts_parents() {
        let dom = dom::parse("<div><p>the and of to the and of to the</p></div>");
        let doc = dom::document(&dom);
        let top = calculate_best_node(&doc).expect("top");
        assert!(get_score(&top) > 0.0);
    }
}
