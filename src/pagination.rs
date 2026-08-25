//! Port of mercury's `next-page-url` extractor (`score-links.js` +
//! `scoring/utils/*` + `utils/text`): score candidate links and pick the
//! most likely next page of a multi-page article.

use crate::dom::{self, Handle};
use crate::regexes;

/// Port of `removeAnchor`.
pub fn remove_anchor(url: &str) -> String {
    url.split('#')
        .next()
        .unwrap_or("")
        .trim_end_matches('/')
        .to_string()
}

/// Port of `pageNumFromUrl`.
pub fn page_num_from_url(url: &str) -> Option<u32> {
    let caps = regexes::page_in_href().captures(url)?;
    let group = caps.get(6)?.as_str();
    let num: u32 = group.parse().ok()?;
    (num < 100).then_some(num)
}

/// Port of `makeBaseRegex` — base URL prefix test.
fn base_prefix_match(href: &str, base_url: &str) -> bool {
    href.to_lowercase().starts_with(&base_url.to_lowercase())
}

/// Port of `makeSig` (linkText + class + id).
fn make_sig(link: &Handle, link_text: &str) -> String {
    format!(
        "{} {} {}",
        link_text,
        dom::class_name(link),
        dom::attr(link, "id").unwrap_or_default()
    )
}

/// Port of `scoreBaseUrl`.
fn score_base_url(href: &str, base_url: &str) -> i32 {
    if !base_prefix_match(href, base_url) {
        -25
    } else {
        0
    }
}

/// Port of `scoreNextLinkText`.
fn score_next_link_text(link_data: &str) -> i32 {
    if regexes::next_link().is_match(link_data) {
        50
    } else {
        0
    }
}

/// Port of `scoreCapLinks`.
fn score_cap_links(link_data: &str) -> i32 {
    if regexes::cap_link_text().is_match(link_data) && regexes::next_link().is_match(link_data) {
        -65
    } else {
        0
    }
}

/// Port of `scorePrevLink`.
fn score_prev_link(link_data: &str) -> i32 {
    if regexes::prev_link().is_match(link_data) {
        -200
    } else {
        0
    }
}

/// Port of `scoreByParents`: up to 4 ancestors scored on page-y/paging-y
/// vs negative class/id signatures.
fn score_by_parents(link: &Handle) -> i32 {
    let mut parent = dom::parent(link);
    let mut positive_match = false;
    let mut negative_match = false;
    let mut score = 0;

    for _ in 0..4 {
        let Some(p) = parent.take() else { break };
        let parent_data = format!(
            "{} {}",
            dom::class_name(&p),
            dom::attr(&p, "id").unwrap_or_default()
        );

        if !positive_match && regexes::page_re().is_match(&parent_data) {
            positive_match = true;
            score += 25;
        }
        if !negative_match
            && regexes::negative_score().is_match(&parent_data)
            && regexes::extraneous_links().is_match(&parent_data)
            && !regexes::positive_score().is_match(&parent_data)
        {
            negative_match = true;
            score -= 25;
        }
        parent = dom::parent(&p);
    }
    score
}

/// Port of `scoreExtraneousLinks`.
fn score_extraneous_links(href: &str) -> i32 {
    if regexes::extraneous_links().is_match(href) {
        -25
    } else {
        0
    }
}

/// Port of `scorePageInLink`.
fn score_page_in_link(page_num: Option<u32>, is_wp: bool) -> i32 {
    if page_num.is_some() && !is_wp {
        50
    } else {
        0
    }
}

/// Port of `scoreLinkText`.
fn score_link_text(link_text: &str, page_num: Option<u32>) -> i32 {
    let mut score = 0;
    if regexes::is_digit().is_match(link_text.trim()) {
        let as_num: u32 = link_text.trim().parse().unwrap_or(0);
        if as_num < 2 {
            score = -30;
        } else {
            score = (10_i32 - as_num as i32).max(0);
        }
        if page_num.is_some_and(|p| p >= as_num) {
            score -= 50;
        }
    }
    score
}

/// Port of `scoreSimilarity`: difflib SequenceMatcher ratio between the
/// article URL and the candidate, on a sliding scale.
fn score_similarity(score: i32, article_url: &str, href: &str) -> i32 {
    if score <= 0 {
        return 0;
    }
    let similarity = seq_ratio(article_url, href);
    let diff_percent = 1.0 - similarity;
    let diff_modifier = -(250.0 * (diff_percent - 0.2));
    score + diff_modifier.round() as i32
}

/// SequenceMatcher-like ratio over chars (difflib `ratio()` = 2*matches/total).
fn seq_ratio(a: &str, b: &str) -> f64 {
    let matches = longest_common_subsequence(a, b) * 2;
    let total = a.chars().count() + b.chars().count();
    if total == 0 {
        1.0
    } else {
        matches as f64 / total as f64
    }
}

fn longest_common_subsequence(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev = vec![0usize; b.len() + 1];
    let mut cur = vec![0usize; b.len() + 1];
    for ca in a {
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = if ca == *cb {
                prev[j] + 1
            } else {
                cur[j].max(prev[j + 1])
            };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Port of `shouldScore`.
#[allow(clippy::too_many_arguments)]
fn should_score(
    href: &str,
    article_url: &str,
    base_url: &str,
    parsed_host: &str,
    link_text: &str,
    previous_urls: &[String],
) -> bool {
    if previous_urls.iter().any(|u| u == href) {
        return false;
    }
    if href.is_empty() || href == article_url || href == base_url {
        return false;
    }
    // Resolve relative hrefs ("/story/2", "?page=2") against the article
    // URL — raw `Url::parse` rejects them and pagination silently died.
    let Ok(link) = url::Url::parse(article_url).and_then(|a| a.join(href)) else {
        return false;
    };
    if link.host_str() != Some(parsed_host) {
        return false;
    }
    // If href doesn't contain a digit after removing the base URL,
    // it's certainly not the next page.
    let fragment = href.strip_prefix(base_url).unwrap_or(href);
    if !regexes::digit().is_match(fragment) {
        return false;
    }

    // This link has extraneous content (like "comment") in its link
    // text, so we skip it.
    if regexes::extraneous_links().is_match(link_text) {
        return false;
    }
    if link_text.chars().count() > 25 {
        return false;
    }
    true
}

/// Port of `articleBaseUrl`: strip pagination-ish segments from a URL to
/// get the article's canonical base.
pub fn article_base_url(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let scheme = parsed.scheme();
    let host = parsed.host_str()?;
    let path = parsed.path();

    let mut first_segment_has_letters = false;
    let mut cleaned: Vec<String> = Vec::new();

    // JS: path.split('/').reverse().reduce((acc, rawSegment, index) => …)
    // — index counts from the *end* of the path.
    let segments: Vec<&str> = path.split('/').collect();
    for (index, raw_segment) in segments.iter().rev().enumerate() {
        let mut segment = (*raw_segment).to_string();

        // Split off and save anything that looks like a file type.
        if segment.contains('.') {
            let mut parts = segment.splitn(2, '.');
            if let (Some(base), Some(ext)) = (parts.next(), parts.next()) {
                if regexes::is_alpha().is_match(ext) {
                    segment = base.to_string();
                }
            }
        }

        // First/second segment (from the end) with a page number: strip it.
        if index < 2 && regexes::page_in_href().is_match(&segment) {
            segment = regexes::page_in_href().replace(&segment, "").into_owned();
        }

        if index == 0 {
            first_segment_has_letters = regexes::has_alpha().is_match(&segment);
        }

        // isGoodSegment(segment, index, firstSegmentHasLetters)
        let mut good_segment = true;
        // (index < 2 && IS_DIGIT && len < 3) is a no-op in the reference.
        if index == 0 && segment.eq_ignore_ascii_case("index") {
            good_segment = false;
        }
        if index < 2 && segment.chars().count() < 3 && !first_segment_has_letters {
            good_segment = false;
        }

        if good_segment {
            cleaned.push(segment);
        }
    }

    cleaned.reverse();
    // Drop the phantom empty segment from the leading `/` (and any
    // similarly empty mid-path segments).
    cleaned.retain(|s| !s.is_empty());
    Some(format!("{scheme}://{host}/{}", cleaned.join("/")))
}

/// Port of `scoreLinks` + the extractor's pick: returns the highest-scored
/// candidate link when its score is strong enough to be confident.
pub fn next_page_url(
    doc: &Handle,
    article_url: &str,
    base_url: &str,
    previous_urls: &[String],
) -> Option<String> {
    let Ok(parsed) = url::Url::parse(article_url) else {
        return None;
    };
    let host = parsed.host_str().unwrap_or("");

    // Port of `isWordpress`: meta[name=generator][value^=WordPress].
    let is_wp = dom::elements_with_attr_value(doc, "name", "generator")
        .iter()
        .any(|n| {
            dom::attr(n, "value")
                .as_deref()
                .is_some_and(|v| v.to_lowercase().starts_with("wordpress"))
        });

    let mut scored: Vec<(String, i32, String)> = Vec::new(); // (href, score, link_text)
    for link in dom::all_nodes_with_tag(doc, &["A"]) {
        let Some(href_raw) = dom::attr(&link, "href") else {
            continue;
        };
        // Resolve relative hrefs against the article URL once, up front —
        // browsers hand mercury absolute `link.href`, and every scorer
        // below (similarity, digit, base-url) expects an absolute URL.
        let href = url::Url::parse(article_url)
            .and_then(|a| a.join(&href_raw))
            .map(|u| u.to_string())
            .unwrap_or_else(|_| href_raw.clone());
        let href = remove_anchor(&href);
        let link_text = dom::inner_text(&link, true);
        if !should_score(
            &href,
            article_url,
            base_url,
            host,
            &link_text,
            previous_urls,
        ) {
            continue;
        }

        let mut score = score_base_url(&href, base_url);
        let link_data = make_sig(&link, &link_text);
        let page_num = page_num_from_url(&href);
        score += score_next_link_text(&link_data);
        score += score_cap_links(&link_data);
        score += score_prev_link(&link_data);
        score += score_by_parents(&link);
        score += score_extraneous_links(&href);
        score += score_page_in_link(page_num, is_wp);
        score += score_link_text(&link_text, page_num);
        score = score_similarity(score, article_url, &href);

        // Merge duplicate hrefs (last one wins, like the reference object).
        if let Some(existing) = scored.iter_mut().find(|(h, _, _)| *h == href) {
            existing.1 = score;
            existing.2 = format!("{}|{}", existing.2, link_text);
        } else {
            scored.push((href, score, link_text));
        }
    }

    // The extractor returns the highest-scored href if its score is >= 50.
    scored
        .into_iter()
        .max_by_key(|(_, score, _)| *score)
        .and_then(|(href, score, _)| (score >= 50).then_some(href))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_num_from_url_extracts() {
        assert_eq!(page_num_from_url("https://x.com/story/page/2"), Some(2));
        assert_eq!(page_num_from_url("https://x.com/story?page=7"), Some(7));
        assert_eq!(page_num_from_url("https://x.com/story"), None);
        assert_eq!(page_num_from_url("https://x.com/story/page/102"), None);
    }

    #[test]
    fn remove_anchor_strips() {
        assert_eq!(remove_anchor("https://x.com/a#top"), "https://x.com/a");
        assert_eq!(remove_anchor("https://x.com/a/"), "https://x.com/a");
    }

    #[test]
    fn finds_next_page_link() {
        let dom = dom::parse(
            "<article><p>story text one two three</p>\
             <div class='pagination'><a href='https://example.com/story/page/1'>1</a>\
             <a href='https://example.com/story/page/2' rel='next'>next</a></div></article>",
        );
        let doc = dom::document(&dom);
        let next = next_page_url(
            &doc,
            "https://example.com/story/page/1",
            "https://example.com/story",
            &[],
        );
        assert_eq!(next.as_deref(), Some("https://example.com/story/page/2"));
    }

    #[test]
    fn follows_relative_next_page_links() {
        // Regression: raw Url::parse rejected relative hrefs, so the
        // multipage feature silently never fired on "/story/2"-style links.
        let dom = dom::parse(
            "<article><p>story text one two three</p>\
             <div class='pagination'><a href='/story/1'>1</a>\
             <a href='/story/2' rel='next'>next</a></div></article>",
        );
        let doc = dom::document(&dom);
        let next = next_page_url(
            &doc,
            "https://example.com/story/1",
            "https://example.com/story",
            &[],
        );
        assert_eq!(next.as_deref(), Some("https://example.com/story/2"));
    }

    #[test]
    fn rejects_cross_domain_links() {
        let dom = dom::parse("<article><a href='https://evil.com/story/page/2'>next</a></article>");
        let doc = dom::document(&dom);
        let next = next_page_url(
            &doc,
            "https://example.com/story",
            "https://example.com/story",
            &[],
        );
        assert_eq!(next, None);
    }

    #[test]
    fn article_base_url_strips_page_numbers() {
        assert_eq!(
            article_base_url("https://x.com/story/page/2").as_deref(),
            Some("https://x.com/story/page")
        );
        assert_eq!(
            article_base_url("https://x.com/story/2").as_deref(),
            Some("https://x.com/story")
        );
        assert_eq!(
            article_base_url("https://x.com/story/page/1.html").as_deref(),
            Some("https://x.com/story/page")
        );
    }

    #[test]
    fn seq_ratio_identity_and_divergence() {
        assert_eq!(seq_ratio("abc", "abc"), 1.0);
        assert_eq!(seq_ratio("abc", "xyz"), 0.0);
        assert!(seq_ratio("https://x.com/story/page/1", "https://x.com/story/page/2") > 0.9);
    }
}
