//! Metadata extraction — ports of newspaper3k's `get_title`, `get_authors`,
//! `get_publishing_date`, `get_meta_*` strategies plus readability's
//! `_getArticleTitle` and mozilla-style meta scanning.

use crate::dom::{self, Handle};
use crate::regexes;
use chrono::{DateTime, NaiveDate, Utc};
use std::collections::HashMap;
use url::Url;

/// Raw metadata collected from the document head/body.
#[derive(Default)]
pub struct Meta {
    /// key -> first value, for `<meta>` elements (name/property/itemprop/http-equiv).
    pub metas: HashMap<String, String>,
    /// rel -> href, for `<link>` elements.
    pub links: HashMap<String, String>,
    pub title_tag: Option<String>,
    pub h1s: Vec<String>,
}

pub fn collect(doc: &Handle) -> Meta {
    let mut meta = Meta::default();
    for el in dom::all_elements(doc) {
        let tag = dom::tag_name(&el);
        match tag.as_deref() {
            Some("META") => {
                let Some(content) = dom::attr(&el, "content") else {
                    continue;
                };
                let property = dom::attr(&el, "property").unwrap_or_default();
                let name = dom::attr(&el, "name").unwrap_or_default();
                // readability `_getArticleMetadata` keying: recognized
                // `prefix:field` tokens from the property list (space-
                // separated values like "x:title dc:title" match too),
                // else the name pattern (dots become colons).
                let key = if let Some(m) = regexes::meta_property().find(&property) {
                    Some(m.as_str().to_lowercase().replace(' ', ""))
                } else if regexes::meta_name().is_match(&name) {
                    Some(name.to_lowercase().replace(' ', "").replace('.', ":"))
                } else {
                    ["property", "name", "itemprop", "http-equiv"]
                        .iter()
                        .find_map(|k| dom::attr(&el, k))
                        .map(|v| v.to_lowercase())
                };
                if let Some(k) = key {
                    if !k.is_empty() {
                        // readability additionally unescapes meta values
                        // (sites double-escape entities); last match wins.
                        meta.metas.insert(k, unescape_html(&content));
                    }
                }
            }
            Some("LINK") => {
                if let (Some(rel), Some(href)) = (dom::attr(&el, "rel"), dom::attr(&el, "href")) {
                    meta.links
                        .entry(rel.to_lowercase())
                        .or_insert_with(|| href.to_string());
                }
            }
            Some("TITLE") => {
                if meta.title_tag.is_none() {
                    meta.title_tag = Some(dom::text_content(&el).trim().to_string());
                }
            }
            Some("H1") => {
                let t = dom::inner_text(&el, true);
                if !t.is_empty() {
                    meta.h1s.push(t);
                }
            }
            _ => {}
        }
    }
    meta
}

pub fn og<'a>(meta: &'a Meta, key: &str) -> Option<&'a str> {
    meta.metas.get(&format!("og:{key}")).map(|s| s.as_str())
}

/// Port of newspaper3k's `get_title` + `split_title`.
#[allow(clippy::if_same_then_else)] // mirrors the Python if/elif structure
pub fn article_title(meta: &Meta) -> String {
    // readability `_getArticleMetadata` precedence: title-ish meta values
    // beat the `<title>` tag.
    for key in [
        "dc:title",
        "dcterm:title",
        "og:title",
        "weibo:article:title",
        "weibo:webpage:title",
        "title",
        "twitter:title",
        "parsely-title",
    ] {
        if let Some(v) = meta.metas.get(key) {
            if !v.trim().is_empty() {
                return v.trim().to_string();
            }
        }
    }

    // readability `_getArticleTitle` on the title tag.
    match meta.title_tag.clone() {
        Some(orig) => readability_title(&orig, &meta.h1s),
        None => meta.h1s.first().cloned().unwrap_or_default(),
    }
}

/// Port of readability's `_getArticleTitle` (separator/colon handling,
/// single-h1 length fallback, 4-word revert).
pub fn readability_title(orig_title: &str, h1s: &[String]) -> String {
    let orig = orig_title.trim().to_string();
    let mut cur = orig.clone();

    let title_had_hierarchical = regexes::title_hier_spaced().is_match(&cur);

    if regexes::title_sep_full().is_match(&cur) {
        // Cut at the LAST separator.
        let last_idx = regexes::title_sep_full()
            .find_iter(&orig)
            .map(|m| m.start())
            .last()
            .unwrap_or(0);
        cur = orig[..last_idx].trim().to_string();
        if word_count(&cur) < 3 {
            // Strip through the FIRST separator instead.
            if let Some(m) = regexes::title_sep_full().find(&orig) {
                cur = orig[m.end()..].trim().to_string();
            }
        }
    } else if cur.contains(": ") {
        // An h1/h2 carrying the exact title means it IS the full title.
        let in_headings = h1s.iter().any(|h| h.trim() == cur.trim());
        if !in_headings {
            cur = orig
                .rfind(": ")
                .map(|i| orig[i + 1..].trim().to_string())
                .unwrap_or_else(|| orig.clone());
            if word_count(&cur) < 3 {
                cur = orig
                    .find(": ")
                    .map(|i| orig[i + 1..].trim().to_string())
                    .unwrap_or_else(|| orig.clone());
            } else if orig
                .find(": ")
                .map(|i| word_count(orig[..i].trim()))
                .unwrap_or(0)
                > 5
            {
                cur = orig.clone();
            }
        }
    } else if (cur.chars().count() > 150 || cur.chars().count() < 15) && h1s.len() == 1 {
        cur = h1s[0].clone();
    }

    cur = regexes::normalize()
        .replace_all(cur.trim(), " ")
        .into_owned();

    let wc = word_count(&cur);
    let words_without_seps = word_count(&regexes::title_sep_full().replace_all(&orig, ""));
    if wc <= 4 && (!title_had_hierarchical || wc != words_without_seps - 1) {
        cur = orig;
    }

    // Product rule (beyond the reference): a short site suffix like
    // "… | Some Blog" is boilerplate — strip it when the headline part
    // stands on its own (≥3 words vs ≤2-word suffix).
    if regexes::title_sep_full().is_match(&cur) {
        if let Some(last) = regexes::title_sep_full().find_iter(&cur).last() {
            let before = cur[..last.start()].trim();
            let after = cur[last.end()..].trim();
            if word_count(before) >= 3 && word_count(after) <= 2 {
                cur = before.to_string();
            }
        }
    }
    cur
}

fn word_count(s: &str) -> usize {
    s.split_whitespace().count()
}

/// UI boilerplate words that never appear in a real byline element.
const BYLINE_JUNK: &[&str] = &[
    "share",
    "save",
    "ago",
    "follow",
    "preferred",
    "sign up",
    "print",
    "bookmark",
];

fn is_junk_byline(content: &str) -> bool {
    let lower = content.to_lowercase();
    BYLINE_JUNK.iter().any(|w| lower.contains(w))
}

pub fn authors(doc: &Handle, extra_byline: Option<&str>) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let attrs = ["name", "rel", "itemprop", "class", "id"];
    let vals = ["author", "byline", "dc.creator", "byl"];
    for a in attrs {
        for v in vals {
            for el in dom::elements_with_attr_value(doc, a, v) {
                let content = if dom::tag_is(&el, "META") {
                    dom::attr(&el, "content").unwrap_or_default()
                } else {
                    dom::text_content(&el)
                };
                // Skip byline containers that are really share/timestamp UI
                // (e.g. BBC's "17 hours ago · Share · Save" block), and
                // over-long containers (readability caps bylines at 100).
                if content.len() >= 100 || is_junk_byline(&content) {
                    continue;
                }
                names.extend(parse_byline(&content));
            }
        }
    }

    // readability `_getArticleMetadata` author path: meta tags keyed by
    // name/property/itemprop (article:author, dc.author, …). By the time
    // this runs the doc has been normalized (property→name, content→value).
    const AUTHOR_META_KEYS: &[&str] = &[
        "article:author",
        "dc:creator",
        "dc.creator",
        "dcterm:creator",
        "dcterm.creator",
        "dc:author",
        "dc.author",
        "author",
        "parsely-author",
        "parsely:author",
        "byl",
        "authors",
    ];
    for el in dom::all_elements(doc) {
        if !dom::tag_is(&el, "META") {
            continue;
        }
        // Exact keys plus a pattern scan for space-separated property
        // values ("dc:creator twitter:site_name") — but only dc/dcterm/
        // article prefixes, never twitter/og handles.
        let key_match = ["name", "property", "itemprop"].iter().any(|a| {
            dom::attr(&el, a)
                .map(|v| v.to_lowercase())
                .is_some_and(|k| {
                    if AUTHOR_META_KEYS.contains(&k.as_str()) {
                        return true;
                    }
                    regexes::meta_property().find(&k).is_some_and(|m| {
                        let key = m.as_str().replace(' ', "");
                        (key.starts_with("dc:")
                            || key.starts_with("dcterm:")
                            || key.starts_with("article:"))
                            && (key.ends_with(":author") || key.ends_with(":creator"))
                    })
                })
        });
        if !key_match {
            continue;
        }
        let content = dom::attr(&el, "content")
            .or_else(|| dom::attr(&el, "value"))
            .unwrap_or_default();
        if !content.is_empty() && content.len() < 100 && !is_junk_byline(&content) {
            // readability trusts meta author content verbatim (URLs included,
            // e.g. "Bradley M. Kuhn (http://ebb.org/bkuhn/)") — only strip a
            // leading "By ".
            let cleaned = crate::mercury::clean_author(&content);
            if !cleaned.is_empty() {
                names.push(cleaned);
            }
        }
    }
    if let Some(b) = extra_byline {
        let b = b.trim();
        if !b.is_empty() && b.len() < 100 {
            names.extend(parse_byline(b));
        }
    }
    uniqify_titlecase(names)
}

/// Role/UI words that are never part of a person's name in a byline.
const NON_NAME_WORDS: &[&str] = &[
    "hours",
    "ago",
    "share",
    "save",
    "add",
    "as",
    "preferred",
    "on",
    "google",
    "technology",
    "reporter",
    "report",
    "editor",
    "writer",
    "contributor",
    "correspondent",
    "follow",
    "print",
    "bookmark",
    "sign",
    "up",
    "posted",
    "written",
    "authoring",
    "am",
    "pm",
    "at",
    "updated",
    "day",
    "week",
    "month",
    "year",
];

fn is_non_name_word(token: &str) -> bool {
    NON_NAME_WORDS.contains(&token.to_lowercase().as_str())
}

/// Port of newspaper's `parse_byline` tokenizer, hardened for real pages:
/// tokens are split at word boundaries AND camelCase boundaries (byline DOM
/// text often arrives glued: "…on GooglePhilippa WainTechnology reporter"),
/// and role/UI words are dropped so only name tokens remain.
fn parse_byline(search_str: &str) -> Vec<String> {
    let mut s = search_str.to_string();
    s = regexes::byline_prefix().replace_all(&s, "").into_owned();
    let raw_tokens: Vec<&str> = regexes::name_split().split(&s).collect();
    // Camel-boundary split: "GooglePhilippa" → ["Google", "Philippa"].
    let mut tokens: Vec<String> = Vec::new();
    for t in raw_tokens {
        if t.chars().any(|c| c.is_ascii_digit()) {
            continue;
        }
        let mut piece = String::new();
        let mut prev: Option<char> = None;
        for (i, c) in t.char_indices() {
            // Camel-boundary split ONLY at a lowercase->uppercase transition
            // ("GooglePhilippa" -> ["Google", "Philippa"]). All-caps runs
            // ("JOE", "O'BRIEN") stay intact -- the old rule split at every
            // uppercase letter and mangled them ("J O E ..."). An all-caps
            // remainder is an acronym glued to a proper noun ("WebMD"), not
            // a camel boundary.
            if prev.is_some_and(|p| p.is_lowercase()) && c.is_uppercase() && !piece.is_empty() {
                let rest_has_lower = t[i..].chars().any(|r| r.is_lowercase());
                if rest_has_lower {
                    tokens.push(std::mem::take(&mut piece));
                }
            }
            piece.push(c);
            prev = Some(c);
        }
        if !piece.is_empty() {
            tokens.push(piece);
        }
    }

    // CJK bylines arrive as one unspaced run of han characters
    // ("作者：肖春芳" → "肖春芳"): accept a ≥2-char pure-CJK name.
    let cjk: Vec<char> = s
        .chars()
        .filter(|c| ('\u{4e00}'..='\u{9fa5}').contains(c))
        .collect();
    if cjk.len() >= 2
        && s.chars()
            .all(|c| c.is_whitespace() || ('\u{4e00}'..='\u{9fa5}').contains(&c) || c == '·')
    {
        return vec![cjk.into_iter().collect()];
    }

    let mut out: Vec<String> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let delimiters = ["and", ",", ""];
    for token in &tokens {
        let token = token.as_str();
        if delimiters.contains(&token) {
            flush_name(&mut current, &mut out);
        } else if !token.chars().any(|c| c.is_ascii_digit())
            && !is_month(token)
            && token.chars().count() <= 12
            && !is_non_name_word(token)
            && current.last().map(String::as_str) != Some(token)
        {
            // Long tokens are UI boilerplate glued together, not names.
            current.push(token.to_string());
        }
    }
    flush_name(&mut current, &mut out);
    out
}

/// Flush accumulated name tokens into an author string, gluing hyphenated
/// line breaks ("Roberts-" + "Grey" → "Roberts-Grey") and merging
/// letter-spaced runs that precede a real word ("B B C" + "News" →
/// "BBC News").
fn flush_name(current: &mut Vec<String>, out: &mut Vec<String>) {
    // Merge a leading run of single-char tokens with a following multi-char
    // word ("B B C" + "News" → "BBC News"). All-single-char runs are left
    // space-joined ("J O E …" stays as-is).
    if current.len() >= 2 && current[0].chars().count() == 1 {
        let mut single_run = 0;
        while single_run < current.len() && current[single_run].chars().count() == 1 {
            single_run += 1;
        }
        if single_run >= 2 && single_run < current.len() && current[single_run].chars().count() > 1
        {
            let merged: String = current[..single_run].concat();
            current.drain(0..single_run);
            current.insert(0, merged);
        }
    }
    if current.len() < 2 {
        current.clear();
        return;
    }
    let mut s = String::new();
    for t in current.iter() {
        if !s.is_empty() && !s.ends_with('-') {
            s.push(' ');
        }
        s.push_str(t);
    }
    out.push(s);
    current.clear();
}

/// Month names that show up in "July 16, 2026 · Author" bylines but are not names.
fn is_month(token: &str) -> bool {
    const MONTHS: &[&str] = &[
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
        "Jan",
        "Feb",
        "Mar",
        "Apr",
        "Jun",
        "Jul",
        "Aug",
        "Sep",
        "Sept",
        "Oct",
        "Nov",
        "Dec",
    ];
    MONTHS.iter().any(|m| m.eq_ignore_ascii_case(token))
}

fn uniqify_titlecase(names: Vec<String>) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    let mut out: Vec<String> = Vec::new();
    for n in names {
        let key = n.to_lowercase();
        if key.is_empty() || seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(titlecase(&n));
    }
    out
}

/// Lowercase particles kept lowercase mid-name ("Jong, Michiel de",
/// "Ludwig van Beethoven"); readability passes meta bylines through verbatim
/// and titlecasing these to "De"/"Van" is a visible corruption.
const NAME_PARTICLES: &[&str] = &[
    "de", "van", "von", "der", "den", "da", "di", "du", "le", "la", "les", "el", "del", "y", "e",
];

fn titlecase(s: &str) -> String {
    s.split_whitespace()
        .enumerate()
        .map(|(idx, w)| {
            let lower = w.to_lowercase();
            if idx > 0 && NAME_PARTICLES.contains(&lower.as_str()) {
                return lower;
            }
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Port of newspaper3k's `get_publishing_date` (URL regex + meta tags), plus
/// `<time datetime>` and JSON-LD `datePublished` fallbacks.
pub fn publish_date(url: Option<&str>, doc: &Handle, meta: &Meta) -> Option<NaiveDate> {
    // 1. Date embedded in the URL.
    if let Some(u) = url {
        if let Some(m) = regexes::url_date().find(u) {
            if let Some(d) = parse_date(&m.as_str().replace(['.', '_', '-', '/'], "-")) {
                return Some(d);
            }
        }
    }
    // 2. Meta tags, in descending reliability (newspaper's list).
    let tag_specs: &[(&str, &str, &str)] = &[
        ("property", "rnews:datePublished", "content"),
        ("property", "article:published_time", "content"),
        ("name", "OriginalPublicationDate", "content"),
        ("itemprop", "datePublished", "datetime"),
        ("property", "og:published_time", "content"),
        ("name", "article_date_original", "content"),
        ("name", "publication_date", "content"),
        ("name", "sailthru.date", "content"),
        ("name", "PublishDate", "content"),
        ("pubdate", "pubdate", "datetime"),
        ("name", "publish_date", "content"),
    ];
    for (attr, value, content_attr) in tag_specs {
        for el in dom::elements_with_attr_value(doc, attr, value) {
            let v = match *content_attr {
                "content" => dom::attr(&el, "content"),
                "datetime" => dom::attr(&el, "datetime"),
                _ => None,
            };
            if let Some(v) = v {
                if let Some(d) = parse_date(&v) {
                    return Some(d);
                }
            }
        }
    }
    // 3. <time datetime="..."> elements.
    for el in dom::all_nodes_with_tag(doc, &["TIME"]) {
        if let Some(dt) = dom::attr(&el, "datetime") {
            if let Some(d) = parse_date(&dt) {
                return Some(d);
            }
        }
    }
    // 4. JSON-LD datePublished (from the raw page text, light-touch).
    if let Some(v) = meta.metas.get("datepublished") {
        if let Some(d) = parse_date(v) {
            return Some(d);
        }
    }
    None
}

/// Parse a JSON-LD `datePublished` value.
pub fn date_from_ld(s: &str) -> Option<NaiveDate> {
    parse_date(s)
}

fn parse_date(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc).date_naive());
    }
    for fmt in [
        "%Y-%m-%d",
        "%Y/%m/%d",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
        "%B %d, %Y",
        "%b %d, %Y",
        "%d %B %Y",
        "%d %b %Y",
        "%B %d %Y",
        "%Y.%m.%d",
    ] {
        if let Ok(d) = NaiveDate::parse_from_str(s, fmt) {
            return Some(d);
        }
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Some(dt.date());
        }
    }
    None
}

/// Find a "Mon DD, YYYY"-style date anywhere in free text.
pub fn date_from_text(s: &str) -> Option<NaiveDate> {
    for m in regexes::text_date().find_iter(s) {
        if let Some(d) = parse_date(m.as_str()) {
            return Some(d);
        }
    }
    None
}

pub fn meta_lang(meta: &Meta, html_lang: Option<&str>, fallback_text: &str) -> Option<String> {
    if let Some(l) = html_lang
        .or_else(|| meta.metas.get("og:locale").map(|s| s.as_str()))
        .or_else(|| meta.metas.get("lang").map(|s| s.as_str()))
        .or_else(|| meta.metas.get("content-language").map(|s| s.as_str()))
    {
        let l = l.trim();
        if !l.is_empty() {
            return Some(l.split(['-', '_']).next().unwrap_or("").to_lowercase());
        }
    }
    whatlang::detect(fallback_text).map(|info| info.lang().code().to_string())
}

pub fn top_image(meta: &Meta, content_root: Option<&Handle>) -> Option<String> {
    for key in ["og:image", "twitter:image", "twitter:image:src"] {
        if let Some(v) = meta.metas.get(key) {
            if !v.is_empty() {
                return Some(v.clone());
            }
        }
    }
    for rel in ["image_src", "icon"] {
        if let Some(v) = meta.links.get(rel) {
            if !v.is_empty() {
                return Some(v.clone());
            }
        }
    }
    if let Some(root) = content_root {
        for img in dom::all_nodes_with_tag(root, &["IMG"]) {
            if let Some(src) = dom::attr(&img, "src") {
                if !src.is_empty() {
                    return Some(src);
                }
            }
        }
    }
    None
}

pub fn images(meta: &Meta, doc: &Handle) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for key in ["og:image", "twitter:image"] {
        if let Some(v) = meta.metas.get(key) {
            if !v.is_empty() {
                out.push(v.clone());
            }
        }
    }
    for img in dom::all_nodes_with_tag(doc, &["IMG"]) {
        if let Some(src) = dom::attr(&img, "src") {
            if !src.is_empty() && !src.starts_with("data:") {
                out.push(src);
            }
        }
    }
    out.dedup();
    out.truncate(25);
    out
}

/// readability's full excerpt precedence: dc:description → dcterm:description
/// → og:description → weibo:* → plain description → twitter:description.
pub fn description(meta: &Meta) -> Option<String> {
    for key in [
        "dc:description",
        "dc.description",
        "dcterm:description",
        "og:description",
        "weibo:article:description",
        "weibo:webpage:description",
        "description",
        "twitter:description",
        "parsely-description",
    ] {
        if let Some(v) = meta.metas.get(key) {
            if !v.trim().is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

pub fn keywords(meta: &Meta) -> Vec<String> {
    meta.metas
        .get("keywords")
        .map(|s| {
            s.split(',')
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

pub fn site_name(meta: &Meta) -> Option<String> {
    og(meta, "site_name")
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

pub fn canonical(meta: &Meta) -> Option<String> {
    meta.links
        .get("canonical")
        .cloned()
        .or_else(|| og(meta, "url").map(|s| s.to_string()))
}

pub fn favicon(meta: &Meta) -> Option<String> {
    for rel in ["icon", "shortcut icon", "apple-touch-icon"] {
        if let Some(v) = meta.links.get(rel) {
            if !v.is_empty() {
                return Some(v.clone());
            }
        }
    }
    None
}

/// Resolve a possibly-relative URL against the article URL.
pub fn resolve(base: Option<&str>, raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    match base.and_then(|b| Url::parse(b).ok()) {
        Some(base_url) => base_url.join(raw).ok().map(|u| u.to_string()),
        None => Url::parse(raw).ok().map(|u| u.to_string()),
    }
}

// ------------------------------------------------------------ JSON-LD

/// Metadata parsed from `<script type="application/ld+json">` blocks —
/// port of readability's `_getJSONLD`.
#[derive(Default)]
pub struct JsonLd {
    pub title: Option<String>,
    pub byline: Option<String>,
    pub excerpt: Option<String>,
    pub site_name: Option<String>,
    pub date_published: Option<String>,
}

/// Port of readability's `_unescapeHtmlEntities` (second decode — real
/// sites double-escape meta values).
pub fn unescape_html(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = s.to_string();
    out = out.replace("&quot;", "\"");
    out = out.replace("&apos;", "'");
    out = out.replace("&#39;", "'");
    out = out.replace("&lt;", "<");
    out = out.replace("&gt;", ">");
    // numeric character references &#123; / &#x1F62D;
    let mut result = String::with_capacity(out.len());
    let mut rest = out.as_str();
    while let Some(pos) = rest.find("&#") {
        result.push_str(&rest[..pos]);
        let after = &rest[pos + 2..];
        let (hex, after) = after
            .strip_prefix(['x', 'X'])
            .map_or((false, after), |a| (true, a));
        let digits: String = after
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .collect();
        if digits.is_empty() || !after[digits.len()..].starts_with(';') {
            // Not a valid numeric reference (e.g. "&#xg;"): keep verbatim,
            // including any hex prefix.
            result.push_str(if hex { "&#x" } else { "&#" });
            rest = after;
            continue;
        }
        let code = u32::from_str_radix(&digits, if hex { 16 } else { 10 });
        if let Ok(code) = code {
            if let Some(c) = char::from_u32(code) {
                if code < 0x20 {
                    // control characters become the replacement char
                    result.push('\u{FFFD}');
                } else {
                    result.push(c);
                }
            } else {
                result.push('\u{FFFD}');
            }
        }
        rest = &after[digits.len() + 1..];
    }
    result.push_str(rest);
    result.replace("&amp;", "&")
}

/// Port of `_getJSONLD(doc)`: scan ld+json scripts for a schema.org article
/// node and pull title/byline/excerpt/siteName/datePublished from it.
/// `html_title` is the already-derived page title, used for the
/// name-vs-headline similarity tiebreak.
pub fn collect_jsonld(doc: &Handle, html_title: &str) -> JsonLd {
    let scripts = dom::all_nodes_with_tag(doc, &["SCRIPT"]);
    for script in scripts {
        if dom::attr(&script, "type").as_deref() != Some("application/ld+json") {
            continue;
        }
        // Strip CDATA markers if present.
        let content = dom::inner_text(&script, false);
        let content = content
            .trim()
            .trim_start_matches("<![CDATA[")
            .trim_end_matches("]]>");
        let Ok(mut parsed) = serde_json::from_str::<serde_json::Value>(content) else {
            continue;
        };

        // Array of nodes: pick the first article-typed one.
        if let Some(arr) = parsed.as_array() {
            let found = arr.iter().find(|it| is_article_type(it));
            match found {
                Some(f) => parsed = f.clone(),
                None => continue,
            }
        }

        if !matches_context(&parsed) {
            continue;
        }

        // No @type but a @graph: find the article-typed node in the graph.
        if parsed.get("@type").is_none() {
            if let Some(graph) = parsed.get("@graph").and_then(|g| g.as_array()) {
                let found = graph.iter().find(|it| is_article_type(it));
                match found {
                    Some(f) => parsed = f.clone(),
                    None => continue,
                }
            }
        }

        if !is_article_type(&parsed) {
            continue;
        }

        let mut ld = JsonLd::default();

        // name vs headline: prefer whichever closely matches the html title.
        let name = parsed.get("name").and_then(|v| v.as_str());
        let headline = parsed.get("headline").and_then(|v| v.as_str());
        ld.title = match (name, headline) {
            (Some(n), Some(h)) if n != h => {
                let name_matches = crate::score::text_similarity(n, html_title) > 0.75;
                let headline_matches = crate::score::text_similarity(h, html_title) > 0.75;
                if headline_matches && !name_matches {
                    Some(h.trim().to_string())
                } else {
                    Some(n.trim().to_string())
                }
            }
            (Some(n), _) => Some(n.trim().to_string()),
            (None, Some(h)) => Some(h.trim().to_string()),
            (None, None) => None,
        };

        if let Some(author) = parsed.get("author") {
            if let Some(name) = author.get("name").and_then(|v| v.as_str()) {
                ld.byline = Some(name.trim().to_string());
            } else if let Some(arr) = author.as_array() {
                let names: Vec<&str> = arr
                    .iter()
                    .filter_map(|a| a.get("name").and_then(|v| v.as_str()).map(str::trim))
                    .collect();
                if !names.is_empty() {
                    ld.byline = Some(names.join(", "));
                }
            }
        }
        if let Some(desc) = parsed.get("description").and_then(|v| v.as_str()) {
            ld.excerpt = Some(desc.trim().to_string());
        }
        if let Some(name) = parsed
            .get("publisher")
            .and_then(|p| p.get("name"))
            .and_then(|v| v.as_str())
        {
            ld.site_name = Some(name.trim().to_string());
        }
        if let Some(date) = parsed.get("datePublished").and_then(|v| v.as_str()) {
            ld.date_published = Some(date.trim().to_string());
        }

        return ld;
    }
    JsonLd::default()
}

fn is_article_type(v: &serde_json::Value) -> bool {
    v.get("@type")
        .and_then(|t| t.as_str())
        .is_some_and(|t| regexes::jsonld_article_types().is_match(t))
}

/// Port of the schema.org `@context` check in `_getJSONLD`.
fn matches_context(v: &serde_json::Value) -> bool {
    match v.get("@context") {
        Some(serde_json::Value::String(s)) => regexes::schema_org().is_match(s),
        Some(obj) => obj
            .get("@vocab")
            .and_then(|v| v.as_str())
            .is_some_and(|s| regexes::schema_org().is_match(s)),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonld_article_parsed() {
        let html = r#"<html><head><title>Fallback Title</title>
            <script type="application/ld+json">
            {"@context":"https://schema.org","@type":"NewsArticle",
             "headline":"Real Headline","name":"Site Name",
             "author":{"name":"Jane Doe"},
             "description":"A short dek.",
             "publisher":{"name":"The Example"},
             "datePublished":"2026-08-01T10:00:00Z"}
            </script></head><body></body></html>"#;
        let dom = dom::parse(html);
        let doc = dom::document(&dom);
        let ld = collect_jsonld(&doc, "Fallback Title");
        assert_eq!(ld.title.as_deref(), Some("Site Name"));
        assert_eq!(ld.byline.as_deref(), Some("Jane Doe"));
        assert_eq!(ld.excerpt.as_deref(), Some("A short dek."));
        assert_eq!(ld.site_name.as_deref(), Some("The Example"));
        assert_eq!(ld.date_published.as_deref(), Some("2026-08-01T10:00:00Z"));
    }

    #[test]
    fn jsonld_headline_wins_on_similarity() {
        // readability's `_textSimilarity`: tokensB not present in tokensA.
        // name-vs-title similarity is low, headline-vs-title is ~1.0.
        let html = r#"<html><head><title>Real Headline of the Article</title>
            <script type="application/ld+json">
            {"@context":"https://schema.org","@type":"Article",
             "name":"Example.com","headline":"Real Headline of the Article"}
            </script></head><body></body></html>"#;
        let dom = dom::parse(html);
        let doc = dom::document(&dom);
        let ld = collect_jsonld(&doc, "Real Headline of the Article");
        assert_eq!(ld.title.as_deref(), Some("Real Headline of the Article"));
    }

    #[test]
    fn jsonld_headline_used_when_no_name() {
        let html = r#"<html><head><title>Real Headline of the Article</title>
            <script type="application/ld+json">
            {"@context":"https://schema.org","@type":"Article",
             "headline":"Real Headline of the Article"}
            </script></head><body></body></html>"#;
        let dom = dom::parse(html);
        let doc = dom::document(&dom);
        let ld = collect_jsonld(&doc, "Real Headline of the Article");
        assert_eq!(ld.title.as_deref(), Some("Real Headline of the Article"));
    }

    #[test]
    fn jsonld_skips_non_article() {
        let html = r#"<html><head>
            <script type="application/ld+json">
            {"@context":"https://schema.org","@type":"WebSite","name":"Nope"}
            </script></head><body></body></html>"#;
        let dom = dom::parse(html);
        let doc = dom::document(&dom);
        let ld = collect_jsonld(&doc, "T");
        assert_eq!(ld.title, None);
    }
}

#[cfg(test)]
mod byline_regression_tests {
    use super::*;

    fn authors_of(html: &str) -> Vec<String> {
        let dom = dom::parse(html);
        let doc = dom::document(&dom);
        authors(&doc, None)
    }

    #[test]
    fn all_caps_byline_survives_camel_split() {
        // Regression: the old camel splitter cut at EVERY uppercase letter,
        // mangling "JOE HILDEBRAND" into "J O E H I L D E B R A N D".
        assert_eq!(
            parse_byline("JOE HILDEBRAND"),
            vec!["JOE HILDEBRAND".to_string()]
        );
        assert_eq!(
            parse_byline("SUSIE O'BRIEN"),
            vec!["SUSIE O'BRIEN".to_string()]
        );
    }

    #[test]
    fn camel_case_gluing_still_splits() {
        // Role words ("on", "reporter") drop; the glued names split.
        assert_eq!(
            parse_byline("on GooglePhilippa WainTechnology reporter"),
            vec!["Philippa Wain".to_string()]
        );
    }

    #[test]
    fn acronym_suffix_stays_glued() {
        // "WebMD" is a proper noun with an all-caps acronym suffix, not a
        // camel boundary.
        let names = parse_byline("Brenda Goodman MA WebMD Health News");
        assert!(names
            .iter()
            .any(|n| n == "Brenda Goodman MA WebMD Health News"));
        assert!(!names.iter().any(|n| n.contains("Web MD")));
    }

    #[test]
    fn dublin_core_dot_creator_meta() {
        // Legacy Dublin Core uses a DOT: <meta name="DC.Creator">. The key
        // table once only had the colon form ("dc:creator") and dropped the
        // author entirely (ietf-1 fixture).
        let names = authors_of(
            r#"<html><head>
            <meta name="DC.Creator" content="Jong, Michiel de" />
            </head><body></body></html>"#,
        );
        assert_eq!(names, vec!["Jong, Michiel de".to_string()]);
    }

    #[test]
    fn accented_byline_no_panic() {
        // Regression: the char-index slicing bug panicked on "V\u{e1}vra"
        // ("end byte index 2 is not a char boundary").
        assert_eq!(
            parse_byline("Ale\u{161} V\u{e1}vra"),
            vec!["Ale\u{161} V\u{e1}vra".to_string()]
        );
    }
}

#[cfg(test)]
mod unescape_tests {
    use super::*;

    #[test]
    fn unescape_named_and_numeric() {
        assert_eq!(unescape_html("&amp; &quot; &lt;"), "& \" <");
        assert_eq!(unescape_html("&#x1F62D;"), "😭");
        assert_eq!(unescape_html("&#128557;"), "😭");
        assert_eq!(unescape_html("&#xFFFFFFFF;"), "\u{FFFD}");
        assert_eq!(unescape_html("&#x0;"), "\u{FFFD}");
        // Invalid references stay verbatim.
        assert_eq!(unescape_html("&#xg;"), "&#xg;");
        assert_eq!(unescape_html("plain"), "plain");
    }

    #[test]
    fn unescape_double_escaped_meta() {
        // The 005 fixture: source has &amp;#x1F62D;, parser gives &#x1F62D;,
        // the second pass decodes to 😭.
        assert_eq!(unescape_html("&#x1F62D;"), "😭");
    }
}
