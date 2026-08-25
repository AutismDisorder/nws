//! Port of readability's `_prepArticle`, `_cleanConditionally`, `_clean`,
//! `_cleanHeaders`, `_markDataTables` and friends.

use crate::dom::{self, Handle, NodeId};
use crate::grab::{Grabber, FLAG_CLEAN_CONDITIONALLY, FLAG_WEIGHT_CLASSES};
use crate::regexes;
use crate::score;

const PRESENTATIONAL_ATTRIBUTES: &[&str] = &[
    "align",
    "background",
    "bgcolor",
    "border",
    "cellpadding",
    "cellspacing",
    "frame",
    "hspace",
    "rules",
    "style",
    "valign",
    "vspace",
];
const DEPRECATED_SIZE_ATTR_ELEMS: &[&str] = &["TABLE", "TH", "TD", "HR", "PRE"];

/// Port of `_prepArticle`.
pub fn prep_article(article_content: &Handle, scores: &score::Scores, g: &Grabber) {
    clean_styles(article_content);
    mark_data_tables(article_content, &g.data_tables);
    crate::post::fix_lazy_images(article_content);
    clean_conditionally(article_content, "form", scores, g);
    clean_conditionally(article_content, "fieldset", scores, g);
    clean(article_content, "object");
    clean(article_content, "embed");
    clean(article_content, "footer");
    clean(article_content, "link");
    clean(article_content, "aside");

    // Share-element cleaning.
    let share_threshold = g.char_threshold;
    let top_candidates = dom::children(article_content);
    for top in &top_candidates {
        clean_matched_nodes(top, |node, match_string| {
            regexes::share_elements().is_match(match_string)
                && dom::text_content(node).len() < share_threshold
        });
    }

    clean(article_content, "iframe");
    clean(article_content, "input");
    clean(article_content, "textarea");
    clean(article_content, "select");
    clean(article_content, "button");
    clean_headers(article_content, scores, g);

    clean_conditionally(article_content, "table", scores, g);
    clean_conditionally(article_content, "ul", scores, g);
    clean_conditionally(article_content, "div", scores, g);

    // h1 -> h2 (h1 is reserved for the page title).
    let h1s = dom::all_nodes_with_tag(article_content, &["H1"]);
    for h1 in h1s {
        dom::set_tag(&h1, "h2");
    }

    // Drop empty paragraphs.
    let ps = dom::all_nodes_with_tag(article_content, &["P"]);
    for p in ps {
        let content_element_count =
            dom::all_nodes_with_tag(&p, &["IMG", "EMBED", "OBJECT", "IFRAME"]).len();
        if content_element_count == 0 && dom::inner_text(&p, false).is_empty() {
            dom::detach(&p);
        }
    }

    // Flatten single-cell tables.
    let tables = dom::all_nodes_with_tag(article_content, &["TABLE"]);
    for table in tables {
        let tbody = if dom::has_single_tag_inside(&table, "TBODY") {
            dom::first_element_child(&table).expect("tbody exists")
        } else {
            table.clone()
        };
        if dom::has_single_tag_inside(&tbody, "TR") {
            let row = dom::first_element_child(&tbody).expect("tr exists");
            if dom::has_single_tag_inside(&row, "TD") {
                let cell = dom::first_element_child(&row).expect("td exists");
                let all_phrasing = dom::child_nodes(&cell).iter().all(dom::is_phrasing_content);
                let new_cell = dom::set_tag(&cell, if all_phrasing { "P" } else { "DIV" });
                dom::replace_node(&table, &new_cell);
            }
        }
    }
}

/// Port of `_cleanStyles`: strip `style` + presentational attributes everywhere.
fn clean_styles(node: &Handle) {
    if dom::tag_name(node).as_deref() == Some("SVG") {
        return;
    }
    for a in PRESENTATIONAL_ATTRIBUTES {
        dom::remove_attr(node, a);
    }
    if let Some(t) = dom::tag_name(node) {
        if DEPRECATED_SIZE_ATTR_ELEMS.contains(&t.as_str()) {
            dom::remove_attr(node, "width");
            dom::remove_attr(node, "height");
        }
    }
    for c in dom::children(node) {
        clean_styles(&c);
    }
}

/// Port of `_markDataTables`.
fn mark_data_tables(
    root: &Handle,
    map: &std::cell::RefCell<std::collections::HashMap<NodeId, bool>>,
) {
    for table in dom::all_nodes_with_tag(root, &["TABLE"]) {
        let id = dom::id(&table);
        if dom::attr(&table, "role").as_deref() == Some("presentation") {
            map.borrow_mut().insert(id, false);
            continue;
        }
        if dom::attr(&table, "datatable").as_deref() == Some("0") {
            map.borrow_mut().insert(id, false);
            continue;
        }
        if dom::attr(&table, "summary").is_some() {
            map.borrow_mut().insert(id, true);
            continue;
        }
        let caption = dom::all_nodes_with_tag(&table, &["CAPTION"])
            .into_iter()
            .next();
        if caption
            .as_ref()
            .is_some_and(|c| !dom::child_nodes(c).is_empty())
        {
            map.borrow_mut().insert(id, true);
            continue;
        }
        let has_data_descendant = ["COL", "COLGROUP", "TFOOT", "THEAD", "TH"]
            .iter()
            .any(|t| !dom::all_nodes_with_tag(&table, &[t]).is_empty());
        if has_data_descendant {
            map.borrow_mut().insert(id, true);
            continue;
        }
        if !dom::all_nodes_with_tag(&table, &["TABLE"]).is_empty() {
            map.borrow_mut().insert(id, false);
            continue;
        }
        let (rows, columns) = row_and_column_count(&table);
        if columns == 1 || rows == 1 {
            map.borrow_mut().insert(id, false);
            continue;
        }
        if rows >= 10 || columns > 4 {
            map.borrow_mut().insert(id, true);
            continue;
        }
        map.borrow_mut().insert(id, rows * columns > 10);
    }
}

fn is_data_table(
    map: &std::cell::RefCell<std::collections::HashMap<NodeId, bool>>,
    node: &Handle,
) -> bool {
    map.borrow().get(&dom::id(node)).copied().unwrap_or(false)
}

/// Port of `_getRowAndColumnCount`.
fn row_and_column_count(table: &Handle) -> (usize, usize) {
    let mut rows = 0usize;
    let mut columns = 0usize;
    for tr in dom::all_nodes_with_tag(table, &["TR"]) {
        let rowspan: usize = dom::attr(&tr, "rowspan")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        rows += rowspan.max(1);
        let mut columns_in_row = 0usize;
        for td in dom::all_nodes_with_tag(&tr, &["TD"]) {
            let colspan: usize = dom::attr(&td, "colspan")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1);
            columns_in_row += colspan.max(1);
        }
        columns = columns.max(columns_in_row);
    }
    (rows, columns)
}

fn has_data_table_ancestor(
    node: &Handle,
    map: &std::cell::RefCell<std::collections::HashMap<NodeId, bool>>,
) -> bool {
    let mut cur = node.clone();
    while let Some(p) = dom::parent(&cur) {
        if dom::tag_is(&p, "TABLE") && is_data_table(map, &p) {
            return true;
        }
        cur = p;
    }
    false
}

/// Port of `_clean(e, tag)`: remove all nodes of a tag unless they embed a video.
fn clean(e: &Handle, tag: &str) {
    let is_embed = matches!(tag, "object" | "embed" | "iframe");
    let upper = tag.to_uppercase();
    let nodes = dom::all_nodes_with_tag(e, &[upper.as_str()]);
    for node in nodes {
        if is_embed && allowed_video(&node) {
            continue;
        }
        dom::detach(&node);
    }
}

fn allowed_video(node: &Handle) -> bool {
    if let Some(attrs) = dom::all_attrs(node) {
        for (_, v) in attrs {
            if regexes::videos().is_match(&v) {
                return true;
            }
        }
    }
    if dom::tag_is(node, "OBJECT") && regexes::videos().is_match(&dom::serialize(node)) {
        return true;
    }
    false
}

/// Port of `_cleanHeaders`.
fn clean_headers(e: &Handle, scores: &score::Scores, g: &Grabber) {
    if !g.flag_is_active(FLAG_WEIGHT_CLASSES) {
        return;
    }
    for node in dom::all_nodes_with_tag(e, &["H1", "H2"]) {
        if score::class_weight(&node) < 0.0 {
            dom::detach(&node);
        }
    }
    let _ = scores;
}

/// Port of `_cleanMatchedNodes`.
fn clean_matched_nodes<F: Fn(&Handle, &str) -> bool>(e: &Handle, filter: F) {
    let end_marker = dom::next_node(e, true);
    let mut next = dom::next_node(e, false);
    while let Some(n) = next {
        if Some(dom::id(&n)) == end_marker.as_ref().map(dom::id) {
            break;
        }
        let match_string = dom::match_string(&n);
        if filter(&n, &match_string) {
            next = dom::next_node(&n, true);
            dom::detach(&n);
        } else {
            next = dom::next_node(&n, false);
        }
    }
}

/// Port of `_cleanConditionally`.
#[allow(clippy::if_same_then_else)] // mirrors the JS else-if structure
fn clean_conditionally(e: &Handle, tag: &str, scores: &score::Scores, g: &Grabber) {
    if !g.flag_is_active(FLAG_CLEAN_CONDITIONALLY) {
        return;
    }
    let _ = scores;
    let upper = tag.to_uppercase();
    for node in dom::all_nodes_with_tag(e, &[upper.as_str()]) {
        let mut is_list = tag == "ul" || tag == "ol";
        if !is_list {
            let mut list_length = 0usize;
            for list in dom::all_nodes_with_tag(&node, &["UL", "OL"]) {
                list_length += dom::inner_text(&list, true).len();
            }
            let node_len = dom::inner_text(&node, true).len();
            is_list = node_len > 0 && (list_length as f64 / node_len as f64) > 0.9;
        }

        if tag == "table" && is_data_table(&g.data_tables, &node) {
            continue;
        }
        if has_data_table_ancestor(&node, &g.data_tables) {
            continue;
        }
        if dom::has_ancestor_tag(&node, "CODE") {
            continue;
        }
        let has_data_tables = dom::all_nodes_with_tag(&node, &["TABLE"])
            .iter()
            .any(|t| is_data_table(&g.data_tables, t));
        if has_data_tables {
            continue;
        }

        let weight = score::class_weight(&node);
        if weight < 0.0 {
            dom::detach(&node);
            continue;
        }

        if score::comma_count(&node) >= 10 {
            continue;
        }

        let p = dom::all_nodes_with_tag(&node, &["P"]).len();
        let img = dom::all_nodes_with_tag(&node, &["IMG"]).len();
        let li = dom::all_nodes_with_tag(&node, &["LI"]).len() as isize - 100;
        let input = dom::all_nodes_with_tag(&node, &["INPUT"]).len();
        let heading_density = score::text_density(&node, &["H1", "H2", "H3", "H4", "H5", "H6"]);

        let mut embed_count = 0usize;
        let mut keep_for_video = false;
        for embed in dom::all_nodes_with_tag(&node, &["OBJECT", "EMBED", "IFRAME"]) {
            if allowed_video(&embed) {
                keep_for_video = true;
                break;
            }
            embed_count += 1;
        }
        if keep_for_video {
            continue;
        }

        let inner_text = dom::inner_text(&node, true);
        if regexes::ad_words().is_match(&inner_text)
            || regexes::loading_words().is_match(&inner_text)
        {
            dom::detach(&node);
            continue;
        }

        let content_length = inner_text.len();
        let link_density = score::link_density(&node);
        let text_density = score::text_density(
            &node,
            &[
                "SPAN",
                "LI",
                "TD",
                "BLOCKQUOTE",
                "DL",
                "DIV",
                "IMG",
                "OL",
                "P",
                "PRE",
                "TABLE",
                "UL",
            ],
        );
        let is_figure_child = dom::has_ancestor_tag(&node, "FIGURE");

        let mut should_remove = false;
        if !is_figure_child && img > 1 && (p as f64 / img as f64) < 0.5 {
            should_remove = true;
        } else if !is_list && li > p as isize {
            should_remove = true;
        } else if input as f64 > (p as f64 / 3.0).floor() {
            should_remove = true;
        } else if !is_list
            && !is_figure_child
            && heading_density < 0.9
            && content_length < 25
            && (img == 0 || img > 2)
            && link_density > 0.0
        {
            should_remove = true;
        } else if !is_list && weight < 25.0 && link_density > 0.2 {
            should_remove = true;
        } else if weight >= 25.0 && link_density > 0.5 {
            should_remove = true;
        } else if (embed_count == 1 && content_length < 75) || embed_count > 1 {
            should_remove = true;
        } else if img == 0 && text_density == 0.0 {
            should_remove = true;
        }

        // Allow simple lists of images.
        if is_list && should_remove {
            let kids = dom::children(&node);
            if kids.iter().any(|c| dom::children(c).len() > 1) {
                dom::detach(&node);
                continue;
            }
            let li_count = dom::all_nodes_with_tag(&node, &["LI"]).len();
            if img == li_count {
                continue;
            }
        }
        if should_remove {
            dom::detach(&node);
        }
    }
}
