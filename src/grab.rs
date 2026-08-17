//! Port of readability's `_grabArticle` — finds the article container in the
//! page by scoring paragraphs and their ancestors.

use crate::clean::prep_article;
use crate::dom::{self, Handle};
use crate::error::{Error, Result};
use crate::regexes;
use crate::score::{self, Scores};
use std::cell::RefCell;
use std::collections::HashMap;

pub const FLAG_STRIP_UNLIKELYS: u8 = 0x1;
pub const FLAG_WEIGHT_CLASSES: u8 = 0x2;
pub const FLAG_CLEAN_CONDITIONALLY: u8 = 0x4;
pub const ALL_FLAGS: u8 = FLAG_STRIP_UNLIKELYS | FLAG_WEIGHT_CLASSES | FLAG_CLEAN_CONDITIONALLY;

const DEFAULT_TAGS_TO_SCORE: &[&str] = &["SECTION", "H2", "H3", "H4", "H5", "H6", "P", "TD", "PRE"];
const UNLIKELY_ROLES: &[&str] = &[
    "menu",
    "menubar",
    "complementary",
    "navigation",
    "alert",
    "alertdialog",
    "dialog",
];
const ALTER_TO_DIV_EXCEPTIONS: &[&str] = &["DIV", "ARTICLE", "SECTION", "P", "OL", "UL"];

/// State shared across the flag-fallback attempts.
pub struct Grabber {
    pub source_html: String,
    pub flags: u8,
    pub char_threshold: usize,
    pub nb_top_candidates: usize,
    pub article_title: String,
    pub article_byline: Option<String>,
    pub article_lang: Option<String>,
    pub data_tables: RefCell<HashMap<dom::NodeId, bool>>,
}

impl Grabber {
    pub fn new(source_html: String, char_threshold: usize, nb_top_candidates: usize) -> Self {
        Grabber {
            source_html,
            flags: ALL_FLAGS,
            char_threshold,
            nb_top_candidates,
            article_title: String::new(),
            article_byline: None,
            article_lang: None,
            data_tables: RefCell::new(HashMap::new()),
        }
    }

    pub fn flag_is_active(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }

    fn remove_flag(&mut self, flag: u8) {
        self.flags &= !flag;
    }

    /// Port of `_grabArticle`. Returns the article container element.
    #[allow(clippy::if_same_then_else)] // mirrors the JS else-if structure
    pub fn grab_article(&mut self) -> Result<Handle> {
        let mut attempts: Vec<(Handle, usize)> = Vec::new();

        loop {
            let dom = dom::parse(&self.source_html);
            let doc = dom::document(&dom);

            let body = dom::all_nodes_with_tag(&doc, &["BODY"])
                .into_iter()
                .next()
                .ok_or(Error::NotExtractable)?;
            let page = body;

            let scores = Scores::new();

            // ---- node walk -------------------------------------------------
            let mut elements_to_score: Vec<Handle> = Vec::new();
            let mut node: Option<Handle> = {
                // start at documentElement (html)
                dom::all_elements(&doc)
                    .into_iter()
                    .find(|n| dom::tag_is(n, "HTML"))
            };
            let mut should_remove_title_header = true;

            while let Some(cur) = node {
                if dom::tag_is(&cur, "HTML") {
                    self.article_lang = dom::attr(&cur, "lang");
                }

                let match_string = dom::match_string(&cur);

                if !dom::is_probably_visible(&cur) {
                    node = dom::next_node(&cur, true);
                    dom::detach(&cur);
                    continue;
                }

                if dom::attr(&cur, "aria-modal").as_deref() == Some("true")
                    && dom::attr(&cur, "role").as_deref() == Some("dialog")
                {
                    node = dom::next_node(&cur, true);
                    dom::detach(&cur);
                    continue;
                }

                // Byline detection + removal (readability `_isValidByline`).
                if self.article_byline.is_none() && is_valid_byline(&cur, &match_string) {
                    let end_marker = dom::next_node(&cur, true);
                    let mut next = dom::next_node(&cur, false);
                    let mut item_prop_name: Option<Handle> = None;
                    while let Some(n) = next {
                        if end_marker.as_ref().map(dom::id) == Some(dom::id(&n)) {
                            break;
                        }
                        if let Some(itemprop) = dom::attr(&n, "itemprop") {
                            if itemprop.contains("name") {
                                item_prop_name = Some(n.clone());
                                break;
                            }
                        }
                        next = dom::next_node(&n, false);
                    }
                    let byline_node = item_prop_name.unwrap_or_else(|| cur.clone());
                    self.article_byline = Some(dom::text_content(&byline_node).trim().to_string());
                    node = dom::next_node(&cur, true);
                    dom::detach(&cur);
                    continue;
                }

                if should_remove_title_header && self.header_duplicates_title(&cur) {
                    should_remove_title_header = false;
                    node = dom::next_node(&cur, true);
                    dom::detach(&cur);
                    continue;
                }

                if self.flag_is_active(FLAG_STRIP_UNLIKELYS) {
                    if regexes::unlikely_candidates().is_match(&match_string)
                        && !regexes::ok_maybe_its_a_candidate().is_match(&match_string)
                        && !dom::has_ancestor_tag(&cur, "TABLE")
                        && !dom::has_ancestor_tag(&cur, "CODE")
                        && !dom::tag_is(&cur, "BODY")
                        && !dom::tag_is(&cur, "A")
                    {
                        node = dom::next_node(&cur, true);
                        dom::detach(&cur);
                        continue;
                    }
                    if dom::attr(&cur, "role")
                        .as_deref()
                        .map(|r| UNLIKELY_ROLES.contains(&r))
                        .unwrap_or(false)
                    {
                        node = dom::next_node(&cur, true);
                        dom::detach(&cur);
                        continue;
                    }
                }

                let tag = dom::tag_name(&cur);
                if let Some(t) = tag.as_deref() {
                    if matches!(
                        t,
                        "DIV" | "SECTION" | "HEADER" | "H1" | "H2" | "H3" | "H4" | "H5" | "H6"
                    ) && dom::is_element_without_content(&cur)
                    {
                        node = dom::next_node(&cur, true);
                        dom::detach(&cur);
                        continue;
                    }
                }

                if let Some(t) = tag.as_deref() {
                    if DEFAULT_TAGS_TO_SCORE.contains(&t) {
                        elements_to_score.push(cur.clone());
                    }

                    // Divs used as paragraphs.
                    if t == "DIV" {
                        // Group consecutive phrasing content into <p>.
                        let mut child = dom::first_child(&cur);
                        while let Some(c) = child {
                            let mut next = dom::next_sibling(&c);
                            if dom::is_phrasing_content(&c) {
                                let mut fragment: Vec<Handle> = Vec::new();
                                let mut current = c.clone();
                                loop {
                                    next = dom::next_sibling(&current);
                                    dom::detach(&current);
                                    fragment.push(current.clone());
                                    match next.clone() {
                                        Some(n) if dom::is_phrasing_content(&n) => current = n,
                                        _ => break,
                                    }
                                }
                                while fragment
                                    .first()
                                    .map(dom::is_whitespace_node)
                                    .unwrap_or(false)
                                {
                                    fragment.remove(0);
                                }
                                while fragment
                                    .last()
                                    .map(dom::is_whitespace_node)
                                    .unwrap_or(false)
                                {
                                    fragment.pop();
                                }
                                if !fragment.is_empty() {
                                    let p = dom::create_element("p");
                                    match &next {
                                        Some(n) => dom::insert_before(&cur, &p, n),
                                        None => dom::append_child(&cur, &p),
                                    }
                                    for f in &fragment {
                                        dom::append_child(&p, f);
                                    }
                                }
                            }
                            child = next;
                        }

                        if dom::has_single_tag_inside(&cur, "P") && score::link_density(&cur) < 0.25
                        {
                            let inner = dom::children(&cur)
                                .into_iter()
                                .next()
                                .expect("single child exists");
                            dom::replace_node(&cur, &inner);
                            elements_to_score.push(inner.clone());
                            // readability: `node = newNode; for-loop increment
                            // calls _getNextNode(node)` — the converted P is
                            // pushed exactly ONCE and never re-processed.
                            // (Re-entering the main walk here double-counted
                            // its score.)
                            node = dom::next_node(&inner, false);
                            continue;
                        } else if !dom::has_child_block_element(&cur) {
                            let replacement = dom::set_tag(&cur, "P");
                            elements_to_score.push(replacement.clone());
                            node = dom::next_node(&replacement, false);
                            continue;
                        }
                    }
                }

                node = dom::next_node(&cur, false);
            }

            // ---- scoring ---------------------------------------------------
            let mut candidates: Vec<Handle> = Vec::new();
            for element_to_score in &elements_to_score {
                let Some(parent_node) = dom::parent(element_to_score) else {
                    continue;
                };
                if dom::tag_name(&parent_node).is_none() {
                    continue;
                }

                let inner_text = dom::inner_text(element_to_score, true);
                if inner_text.len() < 25 {
                    continue;
                }

                let ancestors = get_node_ancestors(element_to_score, Some(5));
                if ancestors.is_empty() {
                    continue;
                }

                let mut content_score = 1.0;
                content_score += score::comma_count(element_to_score) as f64;
                content_score += ((inner_text.len() / 100) as f64).min(3.0);

                for (level, ancestor) in ancestors.iter().enumerate() {
                    if dom::tag_name(ancestor).is_none() || dom::parent(ancestor).is_none() {
                        continue;
                    }
                    if !scores.has(ancestor) {
                        score::initialize_node(
                            &scores,
                            ancestor,
                            self.flag_is_active(FLAG_WEIGHT_CLASSES),
                        );
                        candidates.push(ancestor.clone());
                    }
                    let divider = match level {
                        0 => 1.0,
                        1 => 2.0,
                        l => (l as f64) * 3.0,
                    };
                    scores.add(ancestor, content_score / divider);
                }
            }

            // Top candidates by scaled score.
            let mut top_candidates: Vec<Handle> = Vec::new();
            for candidate in &candidates {
                let candidate_score =
                    scores.get(candidate) * (1.0 - score::link_density(candidate));
                scores.set(candidate, candidate_score);
                let pos = top_candidates
                    .iter()
                    .position(|t| candidate_score > scores.get(t));
                match pos {
                    Some(p) => {
                        top_candidates.insert(p, candidate.clone());
                        if top_candidates.len() > self.nb_top_candidates {
                            top_candidates.pop();
                        }
                    }
                    None if top_candidates.len() < self.nb_top_candidates => {
                        top_candidates.push(candidate.clone());
                    }
                    None => {}
                }
            }

            let mut top_candidate: Option<Handle> = top_candidates.first().cloned();

            if top_candidate.is_none() || dom::tag_is(top_candidate.as_ref().unwrap(), "BODY") {
                // Fallback: wrap the whole page.
                let new_div = dom::create_element("DIV");
                let page_kids = page.children.replace(Vec::new());
                for k in page_kids {
                    dom::append_child(&new_div, &k);
                }
                dom::append_child(&page, &new_div);
                score::initialize_node(&scores, &new_div, self.flag_is_active(FLAG_WEIGHT_CLASSES));
                top_candidate = Some(new_div);
            } else {
                let top = top_candidate.as_ref().unwrap().clone();
                // Alternative candidate ancestors (multi-column articles).
                let mut alternative_candidate_ancestors: Vec<Vec<Handle>> = Vec::new();
                for alt in top_candidates.iter().skip(1) {
                    if scores.get(alt) / scores.get(&top) >= 0.75 {
                        alternative_candidate_ancestors.push(get_node_ancestors(alt, None));
                    }
                }
                const MINIMUM_TOPCANDIDATES: usize = 3;
                if alternative_candidate_ancestors.len() >= MINIMUM_TOPCANDIDATES {
                    let mut parent_of_top = dom::parent(&top);
                    let mut top_new = top.clone();
                    'outer: while let Some(p) = parent_of_top {
                        if dom::tag_is(&p, "BODY") {
                            break;
                        }
                        let mut lists_containing = 0usize;
                        for (i, ancestors) in alternative_candidate_ancestors.iter().enumerate() {
                            if i >= MINIMUM_TOPCANDIDATES {
                                break;
                            }
                            if ancestors.iter().any(|a| dom::id(a) == dom::id(&p)) {
                                lists_containing += 1;
                            }
                        }
                        if lists_containing >= MINIMUM_TOPCANDIDATES {
                            top_new = p.clone();
                            break 'outer;
                        }
                        parent_of_top = dom::parent(&p);
                    }
                    top_candidate = Some(top_new);
                }

                let top = top_candidate.as_ref().unwrap().clone();
                if !scores.has(&top) {
                    score::initialize_node(&scores, &top, self.flag_is_active(FLAG_WEIGHT_CLASSES));
                }

                // Walk up while scores keep rising.
                let mut parent_of_top = dom::parent(&top);
                let mut last_score = scores.get(&top);
                let score_threshold = last_score / 3.0;
                let mut top_new = top.clone();
                while let Some(p) = parent_of_top {
                    if dom::tag_is(&p, "BODY") {
                        break;
                    }
                    if !scores.has(&p) {
                        parent_of_top = dom::parent(&p);
                        continue;
                    }
                    let parent_score = scores.get(&p);
                    if parent_score < score_threshold {
                        break;
                    }
                    if parent_score > last_score {
                        top_new = p.clone();
                        break;
                    }
                    last_score = parent_score;
                    parent_of_top = dom::parent(&p);
                }
                top_candidate = Some(top_new);

                // Unwrap chains of single children.
                let top = top_candidate.as_ref().unwrap().clone();
                let mut parent_of_top = dom::parent(&top);
                let mut top_new = top.clone();
                while let Some(p) = parent_of_top {
                    if dom::tag_is(&p, "BODY") || dom::children(&p).len() != 1 {
                        break;
                    }
                    top_new = p.clone();
                    parent_of_top = dom::parent(&p);
                }
                top_candidate = Some(top_new);

                let top = top_candidate.as_ref().unwrap().clone();
                if !scores.has(&top) {
                    score::initialize_node(&scores, &top, self.flag_is_active(FLAG_WEIGHT_CLASSES));
                }
            }

            // ---- sibling expansion ------------------------------------------
            let article_content = dom::create_element("DIV");
            let top = top_candidate.clone().unwrap();
            let sibling_score_threshold = 10.0f64.max(scores.get(&top) * 0.2);
            let parent_of_top = dom::parent(&top).unwrap_or_else(|| page.clone());
            let mut siblings = dom::children(&parent_of_top);

            let mut s = 0isize;
            while s < siblings.len() as isize {
                let sibling = siblings[s as usize].clone();
                let mut append = false;

                if dom::id(&sibling) == dom::id(&top) {
                    append = true;
                } else {
                    let mut content_bonus = 0.0;
                    if !dom::class_name(&top).is_empty()
                        && dom::class_name(&sibling) == dom::class_name(&top)
                    {
                        content_bonus += scores.get(&top) * 0.2;
                    }
                    // readability guards `sibling.readability &&` — unscored
                    // siblings must not pass the score branch (JS `undefined
                    // + bonus >= threshold` is NaN → false). Without the
                    // guard an unscored same-class sibling got appended.
                    if scores.has(&sibling)
                        && scores.get(&sibling) + content_bonus >= sibling_score_threshold
                    {
                        append = true;
                    } else if dom::tag_is(&sibling, "P") {
                        let link_density = score::link_density(&sibling);
                        let node_content = dom::inner_text(&sibling, true);
                        let node_length = node_content.len();
                        if node_length > 80 && link_density < 0.25 {
                            append = true;
                        } else if node_length < 80
                            && node_length > 0
                            && link_density == 0.0
                            && regexes::sentence_end().is_match(&node_content)
                        {
                            append = true;
                        }
                    }
                }

                if append {
                    let mut sibling = sibling;
                    if let Some(t) = dom::tag_name(&sibling) {
                        if !ALTER_TO_DIV_EXCEPTIONS.contains(&t.as_str()) {
                            sibling = dom::set_tag(&sibling, "DIV");
                        }
                    }
                    dom::append_child(&article_content, &sibling);
                    siblings = dom::children(&parent_of_top);
                    s -= 1;
                }
                s += 1;
            }

            // ---- cleanup + threshold check -----------------------------------
            prep_article(&article_content, &scores, self);

            let text_length = dom::inner_text(&article_content, true).len();
            if text_length >= self.char_threshold {
                return Ok(article_content);
            }

            // Record attempt; degrade flags; retry.
            attempts.push((article_content, text_length));
            if self.flag_is_active(FLAG_STRIP_UNLIKELYS) {
                self.remove_flag(FLAG_STRIP_UNLIKELYS);
            } else if self.flag_is_active(FLAG_WEIGHT_CLASSES) {
                self.remove_flag(FLAG_WEIGHT_CLASSES);
            } else if self.flag_is_active(FLAG_CLEAN_CONDITIONALLY) {
                self.remove_flag(FLAG_CLEAN_CONDITIONALLY);
            } else {
                // No flags left: return the longest attempt.
                attempts.sort_by_key(|a| std::cmp::Reverse(a.1));
                if attempts.first().map(|a| a.1).unwrap_or(0) == 0 {
                    return Err(Error::NotExtractable);
                }
                return Ok(attempts.remove(0).0);
            }
        }
    }

    /// Port of `_headerDuplicatesTitle`.
    fn header_duplicates_title(&self, node: &Handle) -> bool {
        let tag = dom::tag_name(node);
        if tag.as_deref() != Some("H1") && tag.as_deref() != Some("H2") {
            return false;
        }
        let heading = dom::inner_text(node, false);
        score::text_similarity(&self.article_title, &heading) > 0.75
    }
}

/// Port of `_isValidByline`.
fn is_valid_byline(node: &Handle, match_string: &str) -> bool {
    let rel = dom::attr(node, "rel").unwrap_or_default();
    let itemprop = dom::attr(node, "itemprop").unwrap_or_default();
    let byline_length = dom::text_content(node).trim().len();
    (rel == "author" || itemprop.contains("author") || regexes::byline().is_match(match_string))
        && byline_length > 0
        && byline_length < 100
}

/// Port of `_getNodeAncestors` (level 0 = immediate parent).
fn get_node_ancestors(node: &Handle, max_depth: Option<usize>) -> Vec<Handle> {
    let mut ancestors = Vec::new();
    let mut cur = node.clone();
    let mut i = 0usize;
    while let Some(p) = dom::parent(&cur) {
        ancestors.push(p.clone());
        if let Some(max) = max_depth {
            i += 1;
            if i >= max {
                break;
            }
        }
        cur = p;
    }
    ancestors
}
