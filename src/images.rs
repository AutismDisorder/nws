//! Port of mercury's `lead-image-url` extractor (`extractor.js` +
//! `score-image.js` + constants): meta short-circuit, then scored image
//! selection over the article content, then `link[rel=image_src]` fallback.

use crate::dom::{self, Handle};
use crate::regexes;

/// Port of `cleanImage`: trim + must be a valid http(s) URI.
pub fn clean_image(url: &str) -> Option<String> {
    let url = url.trim();
    let parsed = url::Url::parse(url).ok()?;
    match parsed.scheme() {
        "http" | "https" => Some(url.to_string()),
        _ => None,
    }
}

/// Port of `scoreImageUrl`.
fn score_image_url(url: &str) -> i32 {
    let url = url.trim();
    let mut score = 0;
    if regexes::positive_lead().is_match(url) {
        score += 20;
    }
    if regexes::negative_lead().is_match(url) {
        score -= 20;
    }
    if regexes::gif_re().is_match(url) {
        score -= 10;
    }
    if regexes::jpg_re().is_match(url) {
        score += 10;
    }
    score
}

/// Port of `scoreAttr`: alt attribute implies non-presentational.
fn score_attr(img: &Handle) -> i32 {
    if dom::attr(img, "alt")
        .as_deref()
        .is_some_and(|a| !a.is_empty())
    {
        5
    } else {
        0
    }
}

/// Port of `getSig` (class + id).
fn get_sig(node: &Handle) -> String {
    format!(
        "{} {}",
        dom::class_name(node),
        dom::attr(node, "id").unwrap_or_default()
    )
}

/// Port of `scoreByParents`.
fn score_by_parents(img: &Handle) -> i32 {
    let mut score = 0;
    let mut cur = dom::parent(img);
    // figure ancestor anywhere up
    let mut probe = cur.clone();
    let mut found_figure = false;
    while let Some(p) = probe {
        if dom::tag_is(&p, "FIGURE") {
            found_figure = true;
            break;
        }
        probe = dom::parent(&p);
    }
    if found_figure {
        score += 25;
    }

    // parent and grandparent checked against PHOTO_HINTS.
    let parent = cur.take();
    let gparent = parent.as_ref().and_then(dom::parent);
    for node in [parent, gparent].into_iter().flatten() {
        if regexes::photo_hints().is_match(&get_sig(&node)) {
            score += 15;
        }
    }
    score
}

/// Port of `scoreBySibling`: figcaption sibling or photo-hinted sibling.
fn score_by_sibling(img: &Handle) -> i32 {
    let mut score = 0;
    let sibling = dom::next_element_sibling(img);
    if let Some(sibling) = &sibling {
        if dom::tag_is(sibling, "FIGCAPTION") {
            score += 25;
        }
        if regexes::photo_hints().is_match(&get_sig(sibling)) {
            score += 15;
        }
    }
    score
}

/// Port of `scoreByDimensions`.
fn score_by_dimensions(img: &Handle) -> i32 {
    let mut score = 0;
    let width: f64 = dom::attr(img, "width")
        .as_deref()
        .and_then(|w| w.parse().ok())
        .unwrap_or(0.0);
    let height: f64 = dom::attr(img, "height")
        .as_deref()
        .and_then(|h| h.parse().ok())
        .unwrap_or(0.0);
    let src = dom::attr(img, "src").unwrap_or_default();

    if width > 0.0 && width <= 50.0 {
        score -= 50;
    }
    if height > 0.0 && height <= 50.0 {
        score -= 50;
    }
    if width > 0.0 && height > 0.0 && !src.contains("sprite") {
        let area = width * height;
        if area < 5000.0 {
            score -= 100;
        } else {
            score += (area / 1000.0).round() as i32;
        }
    }
    score
}

/// Port of `scoreByPosition`.
fn score_by_position(img_count: usize, index: usize) -> i32 {
    img_count as i32 / 2 - index as i32
}

/// Port of `GenericLeadImageUrlExtractor.extract`, minus the meta
/// short-circuit (handled by `meta::top_image`; pass `article_content`).
pub fn lead_image_from_content(content: &Handle) -> Option<String> {
    let imgs = dom::all_nodes_with_tag(content, &["IMG"]);
    let mut best: Option<(String, i32)> = None;
    for (index, img) in imgs.iter().enumerate() {
        let Some(src) = dom::attr(img, "src") else {
            continue;
        };
        if src.is_empty() {
            continue;
        }
        let mut score = score_image_url(&src);
        score += score_attr(img);
        score += score_by_parents(img);
        score += score_by_sibling(img);
        score += score_by_dimensions(img);
        score += score_by_position(imgs.len(), index);

        if best.as_ref().map(|(_, s)| score > *s).unwrap_or(true) {
            best = Some((src, score));
        }
    }

    let (top_url, top_score) = best?;
    if top_score > 0 {
        if let Some(clean) = clean_image(&top_url) {
            return Some(clean);
        }
    }
    None
}

/// Port of the `link[rel=image_src]` selector fallback.
pub fn lead_image_from_selector(doc: &Handle) -> Option<String> {
    for link in dom::all_nodes_with_tag(doc, &["LINK"]) {
        if dom::attr(&link, "rel")
            .as_deref()
            .is_some_and(|r| r.eq_ignore_ascii_case("image_src"))
        {
            for attr in ["src", "href", "value"] {
                if let Some(v) = dom::attr(&link, attr) {
                    if let Some(clean) = clean_image(&v) {
                        return Some(clean);
                    }
                }
            }
        }
    }
    None
}

/// Full mercury lead-image pipeline: meta tags, then scored content
/// images, then the selector fallback.
pub fn lead_image(doc: &Handle, meta_image: Option<String>, content: &Handle) -> Option<String> {
    if let Some(url) = meta_image.and_then(|u| clean_image(&u)) {
        return Some(url);
    }
    if let Some(url) = lead_image_from_content(content) {
        return Some(url);
    }
    lead_image_from_selector(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_image_url_hints() {
        assert!(score_image_url("https://x.com/wp-content/uploads/big.jpg") > 0);
        assert!(score_image_url("https://x.com/static/sprite.gif") < 0);
    }

    #[test]
    fn lead_image_prefers_meta() {
        let dom = dom::parse(
            "<html><head><meta property='og:image' content='https://x.com/hero.jpg'></head>\
             <body><article><img src='https://x.com/small.png' width='10' height='10'>\
             <figure><img src='https://x.com/big.jpg' width='1200' height='800' alt='main'>\
             <figcaption>caption</figcaption></figure></article></body></html>",
        );
        let doc = dom::document(&dom);
        let article = dom::all_nodes_with_tag(&doc, &["ARTICLE"])[0].clone();
        let via_meta = lead_image(&doc, Some("https://x.com/hero.jpg".into()), &article);
        assert_eq!(via_meta.as_deref(), Some("https://x.com/hero.jpg"));
        // Without meta, the big alt'd captioned image wins over the tiny one.
        let via_scoring = lead_image(&doc, None, &article);
        assert_eq!(via_scoring.as_deref(), Some("https://x.com/big.jpg"));
    }

    #[test]
    fn clean_image_validates_scheme() {
        assert_eq!(
            clean_image("https://x.com/a.jpg"),
            Some("https://x.com/a.jpg".into())
        );
        assert_eq!(clean_image("data:image/png;base64,xx"), None);
        assert_eq!(
            clean_image("  https://x.com/a.jpg  "),
            Some("https://x.com/a.jpg".into())
        );
    }
}
