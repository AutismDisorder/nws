//! Port of readability's `isProbablyReaderable`
//! (`Readability-readerable.js`) — decide whether a document looks like an
//! article *without* running the full extraction. Used by APIs to pre-filter
//! pages before paying for extraction.

use crate::dom::{self, Handle};

/// Options for readerability detection.
#[derive(Debug, Clone, Copy)]
pub struct ReaderableOptions {
    /// Minimum node content length to count.
    pub min_content_length: usize,
    /// Minimum accumulated score to declare the page readerable.
    pub min_score: f64,
}

impl Default for ReaderableOptions {
    fn default() -> Self {
        ReaderableOptions {
            min_content_length: 140,
            min_score: 20.0,
        }
    }
}

/// Port of `isNodeVisible` from readerable.js (inline style display check,
/// `hidden`, `aria-hidden` with the wikimedia `fallback-image` exception).
fn is_node_visible(node: &Handle) -> bool {
    let style = dom::attr(node, "style").unwrap_or_default().to_lowercase();
    // CSSOM comparison upstream is whitespace-insensitive: catch
    // "display: none" as well as "display:none".
    let style_nospace: String = style.chars().filter(|c| !c.is_whitespace()).collect();
    if style_nospace.contains("display:none") || style_nospace.contains("visibility:hidden") {
        return false;
    }
    if dom::has_attr(node, "hidden") {
        return false;
    }
    if let Some(v) = dom::attr(node, "aria-hidden") {
        if v == "true" && !dom::class_name(node).contains("fallback-image") {
            return false;
        }
    }
    true
}

/// Port of `isProbablyReaderable(doc, options)`.
pub fn is_probably_readerable(doc: &Handle, options: &ReaderableOptions) -> bool {
    // nodes = doc.querySelectorAll("p, pre, article")
    let mut nodes: Vec<Handle> = dom::all_nodes_with_tag(doc, &["P", "PRE", "ARTICLE"]);

    // Plus every <div> that has a direct <br> child (br-split articles).
    for br in dom::all_nodes_with_tag(doc, &["BR"]) {
        if let Some(parent) = dom::parent(&br) {
            if dom::tag_is(&parent, "DIV") && !nodes.iter().any(|n| dom::id(n) == dom::id(&parent))
            {
                nodes.push(parent);
            }
        }
    }

    let mut score = 0.0;
    for node in nodes {
        if !is_node_visible(&node) {
            continue;
        }

        let match_string = dom::match_string(&node);
        if crate::regexes::unlikely_candidates().is_match(&match_string)
            && !crate::regexes::ok_maybe_its_a_candidate().is_match(&match_string)
        {
            continue;
        }

        // node.matches("li p") — a <p> anywhere inside a list item is
        // navigation (upstream is a descendant selector, not a direct
        // parent check).
        if dom::tag_is(&node, "P") && has_ancestor_tag(&node, "LI") {
            continue;
        }

        let text_len = dom::text_content(&node).trim().len();
        if text_len < options.min_content_length {
            continue;
        }

        score += ((text_len - options.min_content_length) as f64).sqrt();

        if score > options.min_score {
            return true;
        }
    }
    false
}

/// Convenience: parse `html` and test readerability with defaults.
pub fn is_readerable_html(html: &str) -> bool {
    let dom = dom::parse(html);
    is_probably_readerable(&dom::document(&dom), &ReaderableOptions::default())
}

/// True when any ancestor (any depth) carries the given tag.
fn has_ancestor_tag(node: &Handle, tag: &str) -> bool {
    let mut cur = node.clone();
    while let Some(p) = dom::parent(&cur) {
        if dom::tag_is(&p, tag) {
            return true;
        }
        cur = p;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARTICLE: &str = r#"
    <html><body>
    <article><h1>Title</h1>
    <p>This is the first paragraph of a real article. It contains many
    words, sentences, and a fair amount of substance for the reader to
    consume, as a proper news story would.</p>
    <p>This is the second paragraph of the same article, equally long and
    equally full of real content, more words, more sentences, and more of
    the same substance that makes an article an article.</p>
    <p>Third paragraph, also substantial, with enough words and sentences
    to push the accumulated readerability score comfortably past the
    threshold that the algorithm requires before declaring a page fit
    for extraction purposes.</p>
    </article></body></html>"#;

    #[test]
    fn detects_article() {
        assert!(is_readerable_html(ARTICLE));
    }

    #[test]
    fn rejects_empty_or_nav_pages() {
        assert!(!is_readerable_html("<html><body><p>hi</p></body></html>"));
        assert!(!is_readerable_html(
            "<html><body><ul><li><p>link one</p></li><li><p>link two</p></li></ul></body></html>"
        ));
    }

    #[test]
    fn rejects_hidden_content() {
        // The check is per-node: hide every node that would otherwise count.
        let long_p = "This paragraph is long enough to be counted as content by the readerable check, with many words and sentences in it, over a hundred and forty characters of real text for sure, without any doubt whatsoever at all in the world of extraction heuristics.";
        let html = format!(
            "<html><body><article style='display:none'><p style='display:none'>{long_p}</p></article></body></html>"
        );
        assert!(!is_readerable_html(&html));
    }

    #[test]
    fn detects_br_split_divs() {
        // One node must clear sqrt(len - 140) > 20 alone, so it needs
        // well over 540 chars of text.
        let mut long = String::new();
        for i in 0..30 {
            long.push_str(&format!(
                "Sentence number {i} of this br-split article contains a fair amount of real content, with plenty of words in it. "
            ));
            long.push_str("<br><br>");
        }
        let html = format!("<html><body><div>{long}</div></body></html>");
        assert!(is_readerable_html(&html));
    }

    #[test]
    fn unlikely_candidates_rejected() {
        // The class must sit on every node that would otherwise count.
        let long_p = "This paragraph is long enough to be counted as content by the readerable check, with many words and sentences in it, over a hundred and forty characters of real text for sure, without any doubt whatsoever at all in the world of extraction heuristics.";
        let html = format!(
            "<html><body><article class='comment-section'><p class='comment'>{long_p}</p></article></body></html>"
        );
        assert!(!is_readerable_html(&html));
    }
}
