//! Port of newspaper3k's `cleaners.py` — `DocumentCleaner.clean()` in full:
//! bad-tag removal by id/class/name regex, drop caps, script/style/comment
//! removal, `clean_para_spans`, and `div_to_para` with the left/right
//! link-hoisting buffer walk.

use crate::dom::{self, Handle};
use crate::regexes;

/// Port of `DocumentCleaner.clean()`: remove chunks of the DOM as specified.
pub fn clean_document(doc: &Handle) {
    clean_body_classes(doc);
    clean_article_tags(doc);
    clean_em_tags(doc);
    remove_drop_caps(doc);
    remove_scripts_styles(doc);
    clean_bad_tags(doc);
    remove_nodes_regex(doc, regexes::caption_re());
    remove_nodes_regex(doc, regexes::google_re());
    remove_nodes_regex(doc, regexes::entries_re());
    remove_nodes_regex(doc, regexes::facebook_re());
    remove_nodes_regex(doc, regexes::facebook_broadcasting_re());
    remove_nodes_regex(doc, regexes::twitter_re());
    clean_para_spans(doc);
    div_to_para(doc, "div");
    div_to_para(doc, "span");
    div_to_para(doc, "section");
}

/// Port of `clean_body_classes`.
pub fn clean_body_classes(doc: &Handle) {
    for body in dom::all_nodes_with_tag(doc, &["BODY"]) {
        dom::remove_attr(&body, "class");
    }
}

/// Port of `clean_article_tags`.
pub fn clean_article_tags(doc: &Handle) {
    for article in dom::all_nodes_with_tag(doc, &["ARTICLE"]) {
        dom::remove_attr(&article, "id");
        dom::remove_attr(&article, "name");
        dom::remove_attr(&article, "class");
    }
}

/// Port of `clean_em_tags`: unwrap `<em>` without images inside.
pub fn clean_em_tags(doc: &Handle) {
    for em in dom::all_nodes_with_tag(doc, &["EM"]) {
        if dom::all_nodes_with_tag(&em, &["IMG"]).is_empty() {
            dom::unwrap(&em);
        }
    }
}

/// Port of `remove_drop_caps`: unwrap `span[class~=dropcap]` /
/// `span[class~=drop_cap]` (CSS `~=` is a whitespace-token match).
pub fn remove_drop_caps(doc: &Handle) {
    for span in dom::all_nodes_with_tag(doc, &["SPAN"]) {
        let class = dom::class_name(&span);
        let tokens: Vec<&str> = class.split_whitespace().collect();
        if tokens.contains(&"dropcap") || tokens.contains(&"drop_cap") {
            dom::unwrap(&span);
        }
    }
}

/// Port of `remove_scripts_styles`: scripts, styles and comments out.
pub fn remove_scripts_styles(doc: &Handle) {
    for n in dom::all_nodes(doc) {
        if dom::is_comment(&n) {
            dom::detach(&n);
        }
    }
    for tag in ["SCRIPT", "STYLE"] {
        for n in dom::all_nodes_with_tag(doc, &[tag]) {
            dom::detach(&n);
        }
    }
}

/// Port of the `contains_article` xpath guard: keep the node if it contains
/// an article marker descendant.
fn contains_article(node: &Handle) -> bool {
    !dom::all_nodes_with_tag(node, &["ARTICLE"]).is_empty()
        || dom::all_elements(node).iter().any(|n| {
            dom::attr(n, "id").as_deref() == Some("article")
                || dom::attr(n, "itemprop").as_deref() == Some("articleBody")
        })
}

/// Port of `clean_bad_tags`: remove nodes whose id/class/name matches the
/// naughty list, unless they contain an article marker.
pub fn clean_bad_tags(doc: &Handle) {
    for attr in ["id", "class", "name"] {
        let naughty = dom::all_elements(doc).into_iter().filter(|n| {
            dom::attr(n, attr)
                .as_deref()
                .is_some_and(|v| regexes::naughty().is_match(v))
        });
        for node in naughty {
            // Never detach document-level nodes — a bad class on <html> or
            // <body> (e.g. wikipedia's vector-class list) must not wipe the
            // whole tree; newspaper guards the body class for this reason.
            if dom::tag_is(&node, "HTML")
                || dom::tag_is(&node, "BODY")
                || dom::tag_is(&node, "HEAD")
            {
                continue;
            }
            if !contains_article(&node) {
                dom::detach(&node);
            }
        }
    }
}

/// Port of `remove_nodes_regex`: id/class attributes matched anywhere
/// (case-insensitive) get their elements removed.
pub fn remove_nodes_regex(doc: &Handle, pattern: &regex::Regex) {
    for selector in ["id", "class"] {
        let bad = dom::all_elements(doc).into_iter().filter(|n| {
            dom::attr(n, selector)
                .as_deref()
                .is_some_and(|v| pattern.is_match(v))
        });
        for node in bad {
            dom::detach(&node);
        }
    }
}

/// Port of `clean_para_spans`: unwrap every `<span>` inside a `<p>`.
pub fn clean_para_spans(doc: &Handle) {
    // newspaper `css_select(doc, 'p span')` is a DESCENDANT selector:
    // <p><em><span>…</span></em></p> counts too — the old immediate-parent
    // check left those spans in the cleaned output.
    let spans = dom::all_nodes_with_tag(doc, &["SPAN"])
        .into_iter()
        .filter(|s| {
            let mut cur = s.clone();
            while let Some(p) = dom::parent(&cur) {
                if dom::tag_is(&p, "P") {
                    return true;
                }
                cur = p;
            }
            false
        })
        .collect::<Vec<_>>();
    for span in spans {
        dom::unwrap(&span);
    }
}

// ------------------------------------------------------------- div_to_para

/// The block tags checked by `div_to_para`.
const BLOCK_TAGS: &[&str] = &[
    "A",
    "BLOCKQUOTE",
    "DL",
    "DIV",
    "IMG",
    "OL",
    "P",
    "PRE",
    "TABLE",
    "UL",
];

/// Port of `div_to_para(doc, dom_type)`.
pub fn div_to_para(doc: &Handle, dom_type: &str) {
    let upper = dom_type.to_uppercase();
    let divs = dom::all_nodes_with_tag(doc, &[upper.as_str()]);
    for div in divs {
        // `getElementsByTags` = `descendant::*[self::…]` — descendants only,
        // the node itself is excluded.
        let has_block = dom::all_elements(&div).into_iter().skip(1).any(|n| {
            dom::tag_name(&n)
                .as_deref()
                .is_some_and(|t| BLOCK_TAGS.contains(&t))
        });
        if !has_block {
            // No block content: it is a paragraph in disguise.
            dom::set_tag(&div, "p");
            continue;
        }
        // Gather replacement children: text buffers (with adjacent links
        // hoisted in) flushed as parsed fragments.
        let nodes_to_return = get_replacement_nodes(&div);
        let attrib = dom::all_attrs(&div).unwrap_or_default();

        // `div.clear()` then re-insert.
        for k in dom::child_nodes(&div) {
            dom::detach(&k);
        }
        for n in nodes_to_return {
            dom::append_child(&div, &n);
        }
        for (name, value) in attrib {
            dom::set_attr(&div, &name, &value);
        }
    }
}

/// Port of `get_flushed_buffer` (textToPara → `lxml.html.fromstring`):
/// parse the buffered markup as a fragment; a single root is returned
/// as-is, multiple roots are wrapped in a `<div>` (lxml `create_parent`).
fn text_to_para(text: &str) -> Handle {
    let tmp = dom::parse(text);
    let tmp_doc = dom::document(&tmp);
    let body = dom::all_nodes_with_tag(&tmp_doc, &["BODY"])
        .into_iter()
        .next()
        .unwrap_or_else(|| tmp_doc.clone());
    let kids = dom::child_nodes(&body);
    match kids.len() {
        0 => {
            let p = dom::create_element("p");
            dom::append_child(&p, &dom::create_text(text));
            p
        }
        1 => {
            let node = kids[0].clone();
            dom::detach(&node);
            node
        }
        _ => {
            let div = dom::create_element("div");
            for k in &kids {
                dom::append_child(&div, k);
            }
            div
        }
    }
}

/// Port of `tablines_replacements`: `\n` doubled, tabs dropped, whitespace-
/// only lines removed (ReplaceSequence applies each rule in order).
fn tablines_replacements(text: &str) -> String {
    let doubled = text.replace('\n', "\n\n").replace('\t', "");
    regexes::blank_line().replace_all(&doubled, "").into_owned()
}

/// Port of `replace_walk_left_right`: hoist `<a>` siblings around a text
/// node into the replacement buffer (marked `grv-usedalready`), once each.
fn replace_walk_left_right(
    kid: &Handle,
    kid_text: &str,
    replacement_text: &mut Vec<String>,
    nodes_to_remove: &mut Vec<Handle>,
) {
    let replace_text = tablines_replacements(kid_text);
    if replace_text.chars().count() <= 1 {
        return;
    }

    let mut prev = dom::previous_sibling(kid);
    while let Some(p) = prev {
        let used = dom::attr(&p, "grv-usedalready").as_deref() == Some("yes");
        if dom::tag_is(&p, "A") && !used {
            replacement_text.push(format!(" {} ", dom::link_outer_html(&p)));
            nodes_to_remove.push(p.clone());
            dom::set_attr(&p, "grv-usedalready", "yes");
            prev = dom::previous_sibling(&p);
        } else {
            break;
        }
    }

    replacement_text.push(replace_text);

    let mut next = dom::next_sibling(kid);
    while let Some(n) = next {
        let used = dom::attr(&n, "grv-usedalready").as_deref() == Some("yes");
        if dom::tag_is(&n, "A") && !used {
            replacement_text.push(format!(" {} ", dom::link_outer_html(&n)));
            nodes_to_remove.push(n.clone());
            dom::set_attr(&n, "grv-usedalready", "yes");
            next = dom::next_sibling(&n);
        } else {
            break;
        }
    }
}

/// Port of `get_replacement_nodes`. Removal of hoisted link nodes happens
/// *inside* this function (before re-insertion), exactly as in the reference.
fn get_replacement_nodes(div: &Handle) -> Vec<Handle> {
    let mut replacement_text: Vec<String> = Vec::new();
    let mut nodes_to_return: Vec<Handle> = Vec::new();
    let mut nodes_to_remove: Vec<Handle> = Vec::new();

    for kid in dom::child_nodes(div) {
        if dom::tag_is(&kid, "P") && !replacement_text.is_empty() {
            nodes_to_return.push(text_to_para(&replacement_text.concat()));
            replacement_text.clear();
            nodes_to_return.push(kid);
        } else if dom::is_text(&kid) {
            let kid_text = dom::text_content(&kid);
            replace_walk_left_right(&kid, &kid_text, &mut replacement_text, &mut nodes_to_remove);
        } else {
            nodes_to_return.push(kid);
        }
    }

    if !replacement_text.is_empty() {
        nodes_to_return.push(text_to_para(&replacement_text.concat()));
    }

    for n in nodes_to_remove {
        dom::detach(&n);
    }

    nodes_to_return
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_bad_tags_removes_naughty_nodes() {
        let dom = dom::parse(
            "<body><div class='comment-area'><p>troll</p></div><article><p>keep me</p></article><div class='navbar'>nav</div></body>",
        );
        let doc = dom::document(&dom);
        clean_bad_tags(&doc);
        let text = dom::inner_text(&doc, true);
        assert!(!text.contains("troll"));
        assert!(!text.contains("nav"));
        assert!(text.contains("keep me"));
    }

    #[test]
    fn clean_bad_tags_keeps_article_containers() {
        let dom = dom::parse(
            "<body><div class='comment'><article><p>real article inside</p></article></div></body>",
        );
        let doc = dom::document(&dom);
        clean_bad_tags(&doc);
        let text = dom::inner_text(&doc, true);
        assert!(text.contains("real article inside"));
    }

    #[test]
    fn div_to_para_converts_text_only_divs() {
        let dom = dom::parse(
            "<body><div>plain text with no blocks</div><div><p>real para</p></div></body>",
        );
        let doc = dom::document(&dom);
        div_to_para(&doc, "div");
        let ps = dom::all_nodes_with_tag(&doc, &["P"]);
        assert_eq!(ps.len(), 2);
        assert!(dom::inner_text(&doc, true).contains("plain text"));
    }

    #[test]
    fn div_to_para_hoists_links_around_text() {
        let dom = dom::parse(
            "<body><div><a href='/a'>prev link</a>loose text<a href='/b'>next link</a><p>real paragraph</p></div></body>",
        );
        let doc = dom::document(&dom);
        div_to_para(&doc, "div");
        let text = dom::inner_text(&doc, true);
        assert!(text.contains("loose text"));
        assert!(text.contains("real paragraph"));
        // The links were hoisted into the flushed buffer fragment (parsed
        // from their outerHTML), and the originals re-inserted in place.
        let div = dom::all_nodes_with_tag(&doc, &["DIV"])
            .into_iter()
            .find(|d| dom::parent(d).is_some_and(|p| dom::tag_is(&p, "BODY")))
            .expect("div exists");
        let kids = dom::children(&div);
        assert_eq!(kids.len(), 4);
        assert!(dom::tag_is(&kids[0], "A"));
        assert!(dom::tag_is(&kids[1], "A"));
        assert!(dom::tag_is(&kids[2], "DIV"));
        assert!(dom::tag_is(&kids[3], "P"));
        // The flushed fragment carries both hoisted links + the loose text.
        assert_eq!(dom::all_nodes_with_tag(&kids[2], &["A"]).len(), 2);
        assert!(dom::inner_text(&kids[2], true).contains("loose text"));
    }

    #[test]
    fn remove_drop_caps_unwraps_spans() {
        let dom = dom::parse("<body><p><span class='dropcap'>T</span>he start</p></body>");
        let doc = dom::document(&dom);
        remove_drop_caps(&doc);
        assert!(dom::all_nodes_with_tag(&doc, &["SPAN"]).is_empty());
        assert!(dom::inner_text(&doc, true).contains("The start"));
    }

    #[test]
    fn scripts_styles_comments_removed() {
        let dom = dom::parse(
            "<body><!-- a comment --><style>.x{}</style><script>bad()</script><p>ok</p></body>",
        );
        let doc = dom::document(&dom);
        remove_scripts_styles(&doc);
        assert!(dom::all_nodes_with_tag(&doc, &["SCRIPT"]).is_empty());
        assert!(dom::all_nodes_with_tag(&doc, &["STYLE"]).is_empty());
        assert!(dom::all_nodes(&doc).iter().all(|n| !dom::is_comment(n)));
    }
}
