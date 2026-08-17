//! Port of mercury's value cleaners (`cleaners/title.js`,
//! `cleaners/resolve-split-title.js`, `cleaners/author.js`,
//! `cleaners/date-published.js`) and the generic extractor chains
//! (`extractors/generic/{title,author,date-published,url}`) — used as
//! fallback enrichment when the newspaper strategies come up empty.

use crate::css;
use crate::dom::{self, Handle};
use crate::meta::Meta;
use crate::regexes;
use chrono::{DateTime, Duration, NaiveDate, Utc};

// ---------------------------------------------------------------- selectors

/// Port of `extractFromMeta`: ordered names, exactly one unique non-empty
/// value required.
pub fn extract_from_meta(meta: &Meta, meta_names: &[&str], clean_tags: bool) -> Option<String> {
    for name in meta_names {
        if let Some(v) = meta.metas.get(*name) {
            if v.is_empty() {
                continue;
            }
            let cleaned = if clean_tags { strip_tags(v) } else { v.clone() };
            if !cleaned.is_empty() {
                return Some(cleaned);
            }
        }
    }
    None
}

/// Strip HTML tags from a meta value (mercury `stripTags`).
fn strip_tags(value: &str) -> String {
    if !value.contains('<') {
        return value.trim().to_string();
    }
    let dom = dom::parse(value);
    dom::inner_text(&dom::document(&dom), true)
        .trim()
        .to_string()
}

/// Port of `withinComment`: an ancestor's class or id contains "comment".
fn within_comment(node: &Handle) -> bool {
    let mut cur = dom::parent(node);
    while let Some(p) = cur {
        let sig = format!(
            "{} {}",
            dom::class_name(&p),
            dom::attr(&p, "id").unwrap_or_default()
        );
        if sig.contains("comment") {
            return true;
        }
        cur = dom::parent(&p);
    }
    false
}

/// Port of `extractFromSelectors`: first selector matching exactly one
/// node, with at most `max_children` element children, outside comments.
pub fn extract_from_selectors(
    doc: &Handle,
    selectors: &[&str],
    max_children: usize,
) -> Option<String> {
    for selector in selectors {
        let nodes = css::select_doc(doc, selector);
        if nodes.len() == 1 {
            let node = &nodes[0];
            if dom::children(node).len() > max_children {
                continue;
            }
            if within_comment(node) {
                continue;
            }
            let content = dom::inner_text(node, true);
            if !content.trim().is_empty() {
                return Some(content.trim().to_string());
            }
        }
    }
    None
}

// ------------------------------------------------------------------- title

/// mercury `TITLE_SPLITTERS_RE`.
pub const TITLE_SPLITTERS: &[&str] = &[": ", " - ", " | "];

/// Port of `cleanTitle`.
pub fn clean_title(title: &str, url: &str, h1: Option<&str>) -> String {
    let mut title = title.to_string();
    if TITLE_SPLITTERS.iter().any(|s| title.contains(s)) {
        title = resolve_split_title(&title, url);
    }
    if title.len() > 150 {
        if let Some(h1) = h1 {
            title = h1.to_string();
        }
    }
    normalize_spaces(strip_tags(&title).trim())
}

/// Port of `resolveSplitTitle` (breadcrumb collapsing + domain fuzzy strip).
pub fn resolve_split_title(title: &str, url: &str) -> String {
    // Split while preserving splitters: ["The New York", " - ", "The Post"]
    let mut parts: Vec<String> = Vec::new();
    let mut rest = title.to_string();
    loop {
        let mut next: Option<(usize, usize)> = None;
        for sep in TITLE_SPLITTERS {
            if let Some(pos) = rest.find(sep) {
                let candidate = (pos, sep.len());
                if next
                    .map(|(p, l)| pos < p || (pos == p && candidate.1 > l))
                    .unwrap_or(true)
                {
                    next = Some(candidate);
                }
            }
        }
        match next {
            Some((pos, len)) => {
                parts.push(rest[..pos].to_string());
                parts.push(rest[pos..pos + len].to_string());
                rest = rest[pos + len..].to_string();
            }
            None => {
                if !rest.is_empty() {
                    parts.push(rest);
                }
                break;
            }
        }
    }
    if parts.len() <= 1 {
        return title.to_string();
    }

    // extractBreadcrumbTitle: >= 6 segments with a repeated splitter.
    let splitters: Vec<String> = parts
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, s)| s.clone())
        .collect();
    if parts.len() >= 6 {
        let mut best: Option<(&String, usize)> = None;
        for s in &splitters {
            let count = splitters.iter().filter(|x| *x == s).count();
            if best.map(|(_, c)| count > c).unwrap_or(true) {
                best = Some((s, count));
            }
        }
        if let Some((max_term, term_count)) = best {
            if term_count >= 2 && max_term.chars().count() <= 4 {
                let mut split_on: Vec<String> = Vec::new();
                for chunk in title.split(max_term.as_str()) {
                    split_on.push(chunk.to_string());
                }
                let first = split_on.first().cloned().unwrap_or_default();
                let last = split_on.last().cloned().unwrap_or_default();
                let longest = if first.len() > last.len() {
                    first
                } else {
                    last
                };
                if longest.len() > 10 {
                    return longest;
                }
                return title.to_string();
            }
        }
    }

    // cleanDomainFromTitle: fuzzy-match ends against the naked domain.
    if let Some(new_title) = clean_domain_from_title(&parts, url) {
        return new_title;
    }

    title.to_string()
}

/// Port of `cleanDomainFromTitle` (wuzzy.levenshtein returns a 0..1 ratio).
fn clean_domain_from_title(parts: &[String], url: &str) -> Option<String> {
    let host = url::Url::parse(url).ok()?.host_str()?.to_string();
    let naked_domain = regexes::domain_endings().replace_all(&host, "").to_string();

    let start_slug = parts
        .first()
        .unwrap_or(&String::new())
        .to_lowercase()
        .replace(' ', "");
    let start_ratio = levenshtein_ratio(&start_slug, &naked_domain);
    if start_ratio > 0.4 && start_slug.len() > 5 {
        return Some(parts[2..].join(""));
    }

    let end_slug = parts
        .last()
        .unwrap_or(&String::new())
        .to_lowercase()
        .replace(' ', "");
    let end_ratio = levenshtein_ratio(&end_slug, &naked_domain);
    if end_ratio > 0.4 && end_slug.len() >= 5 {
        return Some(parts[..parts.len().saturating_sub(2)].join(""));
    }

    None
}

/// wuzzy `levenshtein(a, b)` — normalized similarity ratio (0..1).
pub fn levenshtein_ratio(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            cur[j + 1] = (cur[j] + 1).min(prev[j + 1] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    let dist = prev[b.len()];
    1.0 - dist as f64 / a.len().max(b.len()) as f64
}

/// mercury generic title chain: strong meta → strong selectors → weak meta
/// → weak selectors.
pub fn extract_title(doc: &Handle, meta: &Meta, url: &str) -> Option<String> {
    const STRONG_META: &[&str] = &[
        "tweetmeme-title",
        "dc.title",
        "rbtitle",
        "headline",
        "title",
    ];
    const WEAK_META: &[&str] = &["og:title"];
    const STRONG_SELECTORS: &[&str] = &[
        ".hentry .entry-title",
        "h1#articleHeader",
        "h1.articleHeader",
        "h1.article",
        ".instapaper_title",
        "#meebo-title",
    ];
    const WEAK_SELECTORS: &[&str] = &[
        "article h1",
        "#entry-title",
        ".entry-title",
        "#entryTitle",
        "#entrytitle",
        ".entryTitle",
        ".entrytitle",
        "#articleTitle",
        ".articleTitle",
        "post post-title",
        "h1.title",
        "h2.article",
        "h1",
        "html head title",
        "title",
    ];

    let h1 = css::select_doc(doc, "h1")
        .first()
        .map(|h| dom::inner_text(h, true).trim().to_string());

    let title = extract_from_meta(meta, STRONG_META, true)
        .or_else(|| extract_from_selectors(doc, STRONG_SELECTORS, 1))
        .or_else(|| extract_from_meta(meta, WEAK_META, true))
        .or_else(|| extract_from_selectors(doc, WEAK_SELECTORS, 1))?;

    let cleaned = clean_title(&title, url, h1.as_deref());
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

// ------------------------------------------------------------------ author

/// Port of `cleanAuthor`.
pub fn clean_author(author: &str) -> String {
    let cleaned = regexes::clean_author_re()
        .replace(author, "$2")
        .into_owned();
    normalize_spaces(cleaned.trim())
}

/// mercury generic author chain: meta → selectors → byline regexes.
pub fn extract_author(doc: &Handle, meta: &Meta) -> Option<String> {
    const AUTHOR_META: &[&str] = &[
        "byl",
        "clmst",
        "dc.author",
        "dcsext.author",
        "dc.creator",
        "rbauthors",
        "authors",
    ];
    const AUTHOR_MAX_LENGTH: usize = 300;
    const AUTHOR_SELECTORS: &[&str] = &[
        ".entry .entry-author",
        ".author.vcard .fn",
        ".author .vcard .fn",
        ".byline.vcard .fn",
        ".byline .vcard .fn",
        ".byline .by .author",
        ".byline .by",
        ".byline .author",
        ".post-author.vcard",
        ".post-author .vcard",
        "a[rel=author]",
        "#by_author",
        ".by_author",
        "#entryAuthor",
        ".entryAuthor",
        ".byline a[href*=author]",
        "#author .authorname",
        ".author .authorname",
        "#author",
        ".author",
        ".articleauthor",
        ".ArticleAuthor",
        ".byline",
    ];

    let author = extract_from_meta(meta, AUTHOR_META, true)
        .filter(|a| a.len() < AUTHOR_MAX_LENGTH)
        .map(|a| clean_author(&a))
        .or_else(|| {
            extract_from_selectors(doc, AUTHOR_SELECTORS, 2)
                .filter(|a| a.len() < AUTHOR_MAX_LENGTH)
                .map(|a| clean_author(&a))
        });

    if author.is_some() {
        return author;
    }

    // BYLINE_SELECTORS_RE: exactly one #byline/.byline starting with "By".
    for selector in ["#byline", ".byline"] {
        let nodes = css::select_doc(doc, selector);
        if nodes.len() == 1 {
            let text = dom::inner_text(&nodes[0], true);
            if regexes::byline_start().is_match(&text) {
                return Some(clean_author(&text));
            }
        }
    }
    None
}

// -------------------------------------------------------------------- date

/// Port of `cleanDateString` — exact chain: token split, join, meridian
/// dots, digit-anchored meridian spacing (the old unanchored `replace("am",
/// " am ")` corrupted words like "programmatically"), leading "published:"
/// strip.
pub fn clean_date_string(date_string: &str) -> String {
    let joined = regexes::split_date_string()
        .find_iter(date_string)
        .map(|m| m.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let out = regexes::time_meridian_dots().replace_all(&joined, "m");
    let out = regexes::time_meridian_space().replace_all(&out, "$1 $2 $3");
    let out = regexes::clean_date_string().replace(&out, "$1");
    out.trim().to_string()
}

/// Port of `cleanDatePublished`: milliseconds/seconds, offsets, relative
/// times, "now", then full date parsing.
pub fn clean_date_published(date_string: &str) -> Option<NaiveDate> {
    let s = date_string.trim();

    // Milliseconds / seconds since epoch.
    if regexes::ms_date().is_match(s) {
        if let Ok(ms) = s.parse::<i64>() {
            return DateTime::from_timestamp_millis(ms).map(|dt| dt.date_naive());
        }
    }
    if regexes::sec_date().is_match(s) {
        if let Ok(secs) = s.parse::<i64>() {
            return DateTime::from_timestamp(secs, 0).map(|dt| dt.date_naive());
        }
    }

    // "3 hours ago" style. The magnitude is attacker-controlled page text:
    // overflow must yield None (upstream moment() yields invalid → null),
    // never a panic or a fabricated "today".
    if let Some(caps) = regexes::time_ago().captures(s) {
        let n = caps.get(1).and_then(|m| m.as_str().parse::<i64>().ok())?;
        let unit = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let days = match unit {
            u if u.starts_with("second") => n / 86_400,
            u if u.starts_with("minute") => n / 1_440,
            u if u.starts_with("hour") => n / 24,
            u if u.starts_with("day") => n,
            u if u.starts_with("week") => n.saturating_mul(7),
            u if u.starts_with("month") => n.saturating_mul(30),
            u if u.starts_with("year") => n.saturating_mul(365),
            _ => return None,
        };
        // ~1,370 years is beyond any real "ago" — treat as garbage.
        if days.abs() > 500_000 {
            return None;
        }
        let now = Utc::now().checked_sub_signed(Duration::days(days))?;
        return Some(now.date_naive());
    }
    if regexes::time_now().is_match(s) {
        return Some(Utc::now().date_naive());
    }

    // Direct parse, then the cleaned-string retry.
    parse_date(s).or_else(|| {
        let cleaned = clean_date_string(s);
        parse_date(&cleaned)
    })
}

/// Parse a datetime string into a date (ISO 8601 first, then common formats).
fn parse_date(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.date_naive());
    }
    for fmt in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d",
        "%Y/%m/%d",
        "%B %d, %Y",
        "%b %d, %Y",
        "%B %d %Y",
        "%b %d %Y",
        "%d %B %Y",
        "%d %b %Y",
        "%d.%m.%Y",
        "%Y.%m.%d",
        "%m/%d/%Y",
        "%m-%d-%Y",
    ] {
        if let Ok(d) = NaiveDate::parse_from_str(s, fmt) {
            return Some(d);
        }
    }
    // "Aug 15, 2026 at 10:00 am" style — take the date prefix.
    if let Some(m) = regexes::text_date().find(s) {
        let part = m.as_str();
        for fmt in ["%b %d, %Y", "%B %d, %Y", "%b %d %Y", "%B %d %Y"] {
            if let Ok(d) = NaiveDate::parse_from_str(&part.replace(",", ""), fmt) {
                return Some(d);
            }
        }
    }
    None
}

/// mercury generic date chain: meta → selectors → URL regexes.
pub fn extract_date(doc: &Handle, meta: &Meta, url: &str) -> Option<NaiveDate> {
    const DATE_META: &[&str] = &[
        "article:published_time",
        "displaydate",
        "dc.date",
        "dc.date.issued",
        "rbpubdate",
        "publish_date",
        "pub_date",
        "pagedate",
        "pubdate",
        "revision_date",
        "doc_date",
        "date_created",
        "content_create_date",
        "lastmodified",
        "created",
        "date",
    ];
    const DATE_SELECTORS: &[&str] = &[
        ".hentry .dtstamp.published",
        ".hentry .published",
        ".hentry .dtstamp.updated",
        ".hentry .updated",
        ".single .published",
        ".meta .published",
        ".meta .postDate",
        ".entry-date",
        ".byline .date",
        ".postmetadata .date",
        ".article_datetime",
        ".date-header",
        ".story-date",
        ".dateStamp",
        "#story .datetime",
        ".dateline",
        ".pubdate",
    ];

    extract_from_meta(meta, DATE_META, false)
        .and_then(|d| clean_date_published(&d))
        .or_else(|| {
            extract_from_selectors(doc, DATE_SELECTORS, 1).and_then(|d| clean_date_published(&d))
        })
        .or_else(|| {
            for re in [
                regexes::url_yyyymmdd_slash(),
                regexes::url_yyyymmdd_dash(),
                regexes::url_yyyymm_mon(),
            ] {
                if let Some(m) = re.find(url) {
                    if let Some(d) = clean_date_published(m.as_str()) {
                        return Some(d);
                    }
                }
            }
            None
        })
}

/// Port of `normalizeSpaces` (preserving pre/code/textarea whitespace).
/// The JS is `text.replace(/\s{2,}(?![^<>]*<\/(pre|code|textarea)>)/g, ' ')`:
/// whitespace runs of 2+ are collapsed to one space unless the remainder
/// matches `[^<>]*</(pre|code|textarea)>` (inside a code block).
pub fn normalize_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut iter = text.chars().peekable();
    while let Some(c) = iter.next() {
        if c.is_whitespace() {
            let mut run = String::new();
            run.push(c);
            while let Some(&n) = iter.peek() {
                if n.is_whitespace() {
                    run.push(n);
                    iter.next();
                } else {
                    break;
                }
            }
            if run.chars().count() >= 2 {
                let remainder: String = iter.clone().collect();
                if regexes::pre_close().is_match(&remainder) {
                    out.push_str(&run);
                } else {
                    out.push(' ');
                }
            } else {
                out.push_str(&run);
            }
        } else {
            out.push(c);
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod date_regression_tests {
    use super::*;

    #[test]
    fn hostile_relative_dates_never_panic_or_fabricate() {
        // The old code did `n * 365` + `Utc::now() - delta` — overflow in
        // release wraps, chrono's Sub panics, and unparseable magnitudes
        // returned TODAY. All must now yield None.
        assert_eq!(clean_date_published("99999999999999999999 years ago"), None);
        assert_eq!(clean_date_published("99999999999999999999 days ago"), None);
        assert_eq!(clean_date_published("1234567890123 months ago"), None);
        assert_eq!(clean_date_published("1000000 years ago"), None);
    }

    #[test]
    fn sane_relative_dates_still_parse() {
        let today = Utc::now().date_naive();
        let d = clean_date_published("1 day ago").expect("parses");
        assert_eq!(d, today - chrono::Duration::days(1));
        let d = clean_date_published("2 hours ago").expect("parses");
        assert_eq!(d, today);
        let d = clean_date_published("5 years ago").expect("parses");
        assert_eq!(d, today - chrono::Duration::days(5 * 365));
    }

    #[test]
    fn meridian_spacing_is_digit_anchored() {
        // Mercury's chain: tokens are date-shaped only, and the meridian
        // spacing anchors on a preceding digit (unanchored replace would
        // also hit stray "am"/"pm" substrings inside larger tokens).
        assert_eq!(clean_date_string("at 10:30am sharp"), "10:30 am");
        // The meridian is part of the colon-time token only.
        assert_eq!(
            clean_date_string("published: Aug 15 2026 2:00pm"),
            "Aug 15 2026 2:00 pm"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_author_strips_byline() {
        assert_eq!(clean_author("By Jane Doe"), "Jane Doe");
        assert_eq!(clean_author("written by Jane Doe"), "Jane Doe");
        assert_eq!(clean_author("posted by: Jane Doe"), "Jane Doe");
    }

    #[test]
    fn clean_title_strips_domain_suffix() {
        let t = clean_title(
            "Real Article Headline - Example News",
            "https://example.com/story",
            None,
        );
        assert_eq!(t, "Real Article Headline");
    }

    #[test]
    fn resolve_split_title_breadcrumbs() {
        let title = "The Best Gadgets : Bits : Blogs : The Best Gadgets : Bits : Blogs";
        let resolved = resolve_split_title(title, "https://nytimes.com");
        assert!(resolved.len() < title.len());
    }

    #[test]
    fn clean_date_published_variants() {
        assert!(clean_date_published("2026-08-15T10:00:00Z").is_some());
        assert!(clean_date_published("2026-08-15").is_some());
        assert!(clean_date_published("August 15, 2026").is_some());
        assert!(clean_date_published("3 days ago").is_some());
        assert!(clean_date_published("just now").is_some());
        assert_eq!(clean_date_published("not a date"), None);
    }

    #[test]
    fn extract_author_from_selectors() {
        let dom = dom::parse(
            "<html><body><div class='byline'><span class='author'>By Jane Doe</span></div></body></html>",
        );
        let doc = dom::document(&dom);
        let meta = Meta::default();
        assert_eq!(extract_author(&doc, &meta).as_deref(), Some("Jane Doe"));
    }

    #[test]
    fn extract_title_chain() {
        let dom = dom::parse(
            "<html><head><title>Fallback</title></head><body><article><h1 class='entry-title'>Real Title</h1></article></body></html>",
        );
        let doc = dom::document(&dom);
        let meta = Meta::default();
        let t = extract_title(&doc, &meta, "https://example.com");
        assert_eq!(t.as_deref(), Some("Real Title"));
    }

    #[test]
    fn levenshtein_ratio_basics() {
        assert!((levenshtein_ratio("example", "example") - 1.0).abs() < 1e-9);
        assert!(levenshtein_ratio("example", "examplx") > 0.8);
        assert!(levenshtein_ratio("abc", "xyz") < 0.5);
    }

    #[test]
    fn normalize_spaces_preserves_pre() {
        assert_eq!(normalize_spaces("a   b"), "a b");
        let s = normalize_spaces("x  \n  </pre>");
        assert!(s.contains("  "), "whitespace before </pre> preserved");
    }
}
