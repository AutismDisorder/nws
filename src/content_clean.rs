//! Port of mercury's content post-cleaning (`cleaners/content.js` and the
//! `utils/dom` helpers it calls): rewriteTopLevel, cleanImages, markToKeep,
//! stripJunkTags, cleanHOnes, cleanHeaders, cleanTags, removeEmpty,
//! cleanAttributes — applied to the extracted article before serialization.

use crate::dom::{self, Handle};
use crate::regexes;
use crate::score;

/// The class mercury uses to mark elements that must survive stripping.
const KEEP_CLASS: &str = "mercury-parser-keep";

/// Port of `KEEP_SELECTORS`: video embeds to preserve.
const KEEP_SELECTORS: &[&str] = &[
    "https://www.youtube.com",
    "https://www.youtube-nocookie.com",
    "http://www.youtube.com",
    "https://player.vimeo",
    "http://player.vimeo",
    "https://www.redditmedia.com",
];

/// Port of `STRIP_OUTPUT_TAGS`: tags removed from the final output.
const STRIP_OUTPUT_TAGS: &[&str] = &[
    "TITLE", "SCRIPT", "NOSCRIPT", "LINK", "STYLE", "HR", "EMBED", "IFRAME", "OBJECT",
];

/// Port of `WHITELIST_ATTRS`: attributes that survive the attribute pass.
const WHITELIST_ATTRS: &[&str] = &[
    "src",
    "srcset",
    "sizes",
    "type",
    "href",
    "class",
    "id",
    "alt",
    "xlink:href",
    "width",
    "height",
];

/// Port of mercury `extractCleanNode`: the full post-extraction cleaning
/// pipeline applied to the article container.
pub fn clean_content(article: &Handle, title: &str, url: &str) {
    rewrite_top_level(article);
    clean_images(article);
    crate::post::fix_relative_uris(article, Some(url));
    mark_to_keep(article, url);
    strip_junk_tags(article);
    clean_h_ones(article);
    clean_headers(article, title);
    clean_tags(article);
    remove_empty(article);
    clean_attributes(article);
}

/// Port of `rewriteTopLevel`: html/body as the article node become div.
fn rewrite_top_level(article: &Handle) {
    if dom::tag_is(article, "HTML") || dom::tag_is(article, "BODY") {
        dom::set_tag(article, "div");
    }
}

/// Port of `cleanImages`: drop tiny and spacer images, unset explicit height.
/// Extends the reference spacer check with `placeholder` sources and the
/// generic `aria-label="image unavailable"` marker (BBC-style noscript
/// placeholders — the references all keep these; we remove them for output
/// cleanliness).
fn clean_images(article: &Handle) {
    let imgs = dom::all_nodes_with_tag(article, &["IMG"]);
    for img in imgs {
        let height: i64 = dom::attr(&img, "height")
            .as_deref()
            .and_then(|h| h.parse().ok())
            .unwrap_or(0);
        let width: i64 = dom::attr(&img, "width")
            .as_deref()
            .and_then(|w| w.parse().ok())
            .unwrap_or(20);

        // mercury: `(height || 20) < 10` — remove only when a *parsed*
        // height is between 1 and 9 (shims/icons); an absent height parses
        // to 0 and must NOT be treated as small. (The old `height.max(20)`
        // made this branch dead code.)
        if (height > 0 && height < 10) || width < 10 {
            dom::detach(&img);
            continue;
        } else if height > 0 {
            // Never fix a height on images: scale by width.
            dom::remove_attr(&img, "height");
        }

        if dom::attr(&img, "aria-label").as_deref() == Some("image unavailable") {
            dom::detach(&img);
            continue;
        }

        if dom::attr(&img, "src")
            .as_deref()
            .is_some_and(|s| regexes::spacer().is_match(s) || s.contains("placeholder"))
        {
            dom::detach(&img);
        }
    }
}

/// Port of `markToKeep`: tag video embeds (and same-host iframes) with the
/// keep class so `stripJunkTags` leaves them alone.
fn mark_to_keep(article: &Handle, url: &str) {
    let same_host = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| format!("{}://{}", u.scheme(), h)));

    let iframes = dom::all_nodes_with_tag(article, &["IFRAME"]);
    for iframe in iframes {
        let src = dom::attr(&iframe, "src").unwrap_or_default();
        let keep = KEEP_SELECTORS.iter().any(|k| src.starts_with(k))
            || same_host.as_deref().is_some_and(|h| src.starts_with(h));
        if keep {
            let mut class = dom::class_name(&iframe);
            if !class.split_whitespace().any(|c| c == KEEP_CLASS) {
                class.push_str(if class.is_empty() { "" } else { " " });
                class.push_str(KEEP_CLASS);
                dom::set_attr(&iframe, "class", &class);
            }
        }
    }
}

fn has_keep_class(node: &Handle) -> bool {
    dom::class_name(node)
        .split_whitespace()
        .any(|c| c == KEEP_CLASS)
}

/// Port of `stripJunkTags`: remove `STRIP_OUTPUT_TAGS` unless marked keep.
fn strip_junk_tags(article: &Handle) {
    for tag in STRIP_OUTPUT_TAGS {
        for node in dom::all_nodes_with_tag(article, &[tag]) {
            let keep = has_keep_class(&node) || dom::all_elements(&node).iter().any(has_keep_class);
            if !keep {
                dom::detach(&node);
            }
        }
    }
}

/// Port of `cleanHOnes`: <3 H1s → strip; 3+ → convert to H2.
fn clean_h_ones(article: &Handle) {
    let h_ones = dom::all_nodes_with_tag(article, &["H1"]);
    if h_ones.len() < 3 {
        for h in h_ones {
            dom::detach(&h);
        }
    } else {
        for h in h_ones {
            dom::set_tag(&h, "h2");
        }
    }
}

/// Port of `cleanHeaders`: drop headers before any paragraph, headers
/// duplicating the title, and negative-weight headers.
fn clean_headers(article: &Handle, title: &str) {
    for header in dom::all_nodes_with_tag(article, &["H2", "H3", "H4", "H5", "H6"]) {
        // `prevAll('p')`: any *preceding sibling* paragraph means real
        // content started; headers before all paragraphs are junk.
        let mut has_p_before = false;
        let mut sib = dom::previous_element_sibling(&header);
        while let Some(s) = sib {
            if dom::tag_is(&s, "P") {
                has_p_before = true;
                break;
            }
            sib = dom::previous_element_sibling(&s);
        }
        if !has_p_before {
            dom::detach(&header);
            continue;
        }

        let text = dom::inner_text(&header, true);
        if !title.is_empty() && text == title {
            dom::detach(&header);
            continue;
        }

        if score::class_weight(&header) < 0.0 {
            dom::detach(&header);
        }
    }
}

/// Port of `removeUnlessContent` (mercury `cleanTags`).
fn remove_unless_content(node: &Handle, weight: f64) -> bool {
    // entry-content-asset is valuable per publisher guidelines.
    if dom::class_name(node)
        .split_whitespace()
        .any(|c| c == "entry-content-asset")
    {
        return false;
    }

    let content = collapse(&dom::inner_text(node, true));
    if score::comma_count_text(&content) < 10 {
        let p_count = dom::all_nodes_with_tag(node, &["P"]).len();
        let input_count = dom::all_nodes_with_tag(node, &["INPUT"]).len();

        // Looks like a form, too many inputs.
        if input_count as f64 > p_count as f64 / 3.0 {
            return true;
        }

        let content_length = content.len();
        let img_count = dom::all_nodes_with_tag(node, &["IMG"]).len();

        // readability's short-content rule requires a positive link density
        // (`contentLength < 25 && (img === 0 || img > 2) && linkDensity > 0`);
        // mercury's unconditional version nukes tiny-but-real articles.
        let density = score::link_density(node);
        if content_length < 25 && (img_count == 0 || img_count > 2) && density > 0.0 {
            return true;
        }

        if weight < 25.0 && density > 0.2 && content_length > 75 {
            return true;
        }

        if weight >= 25.0 && density > 0.5 {
            let tag = dom::tag_name(node).unwrap_or_default();
            let node_is_list = tag == "OL" || tag == "UL";
            if node_is_list {
                // Keep list when the previous sibling ends with a colon.
                if let Some(prev) = dom::previous_element_sibling(node) {
                    if collapse(&dom::inner_text(&prev, true)).ends_with(':') {
                        return false;
                    }
                }
            }
            return true;
        }

        let script_count = dom::all_nodes_with_tag(node, &["SCRIPT"]).len();
        if script_count > 0 && content_length < 150 {
            return true;
        }
    }
    false
}

/// Port of `cleanTags`: conditionally-clean ul/ol/table/div/button/form.
fn clean_tags(article: &Handle) {
    let tags = ["UL", "OL", "TABLE", "DIV", "BUTTON", "FORM"];
    for node in dom::all_nodes_with_tag(article, &tags) {
        if has_keep_class(&node) || dom::all_elements(&node).iter().any(has_keep_class) {
            continue;
        }
        let weight = score::class_weight(&node);
        if weight < 0.0 || remove_unless_content(&node, weight) {
            dom::detach(&node);
        }
    }
}

/// Port of `removeEmpty`: empty paragraphs without media go away.
fn remove_empty(article: &Handle) {
    for p in dom::all_nodes_with_tag(article, &["P"]) {
        let has_media = !dom::all_nodes_with_tag(&p, &["IFRAME", "IMG"]).is_empty();
        if !has_media && dom::inner_text(&p, true).trim().is_empty() {
            dom::detach(&p);
        }
    }
}

/// Port of `cleanAttributes`: keep only whitelisted attributes, drop the
/// keep class afterwards.
fn clean_attributes(article: &Handle) {
    for node in dom::all_elements(article) {
        let Some(attrs) = dom::all_attrs(&node) else {
            continue;
        };
        for (name, _) in &attrs {
            if !WHITELIST_ATTRS.iter().any(|w| w.eq_ignore_ascii_case(name)) {
                dom::remove_attr(&node, name);
            }
        }
        // Remove the keep class from the final result.
        if has_keep_class(&node) {
            let class = dom::class_name(&node);
            let kept: Vec<&str> = class
                .split_whitespace()
                .filter(|c| *c != KEEP_CLASS)
                .collect();
            if kept.is_empty() {
                dom::remove_attr(&node, "class");
            } else {
                dom::set_attr(&node, "class", &kept.join(" "));
            }
        }
    }
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod small_image_tests {
    use super::*;

    fn imgs_after(html: &str) -> usize {
        let dom = dom::parse(html);
        let doc = dom::document(&dom);
        clean_images(&doc);
        dom::all_nodes_with_tag(&doc, &["IMG"]).len()
    }

    #[test]
    fn parsed_tiny_height_removed() {
        // height=5 → mercury `(height || 20) < 10` → removed.
        assert_eq!(
            imgs_after("<div><img src='a.png' width='100' height='5'></div>"),
            0
        );
    }

    #[test]
    fn absent_height_kept() {
        // No height attribute → must survive (old `height.max(20)` dead code
        // only mattered here; correct rule keeps it).
        assert_eq!(imgs_after("<div><img src='a.png' width='100'></div>"), 1);
    }

    #[test]
    fn height_zero_kept() {
        assert_eq!(
            imgs_after("<div><img src='a.png' width='100' height='0'></div>"),
            1
        );
    }

    #[test]
    fn small_width_removed() {
        assert_eq!(imgs_after("<div><img src='a.png' width='8'></div>"), 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_junk_keeps_marked_videos() {
        let dom = dom::parse(
            "<div><p>text one two three four five six seven eight nine ten eleven twelve</p>\
             <iframe src='https://www.youtube.com/embed/x'></iframe><hr>\
             <iframe src='https://ads.example.com/widget'></iframe></div>",
        );
        let div = dom::all_nodes_with_tag(&dom::document(&dom), &["DIV"])[0].clone();
        clean_content(&div, "", "https://example.com/story");
        let iframes = dom::all_nodes_with_tag(&div, &["IFRAME"]);
        assert_eq!(iframes.len(), 1);
        assert!(dom::attr(&iframes[0], "src")
            .as_deref()
            .unwrap()
            .contains("youtube"));
        assert!(dom::all_nodes_with_tag(&div, &["HR"]).is_empty());
    }

    #[test]
    fn clean_images_drops_spacers() {
        let dom = dom::parse(
            "<div><img src='https://x.com/spacer.gif'><img src='https://x.com/a.jpg' width='600' height='400'></div>",
        );
        let div = dom::all_nodes_with_tag(&dom::document(&dom), &["DIV"])[0].clone();
        clean_images(&div);
        let imgs = dom::all_nodes_with_tag(&div, &["IMG"]);
        assert_eq!(imgs.len(), 1);
        assert_eq!(
            dom::attr(&imgs[0], "height"),
            None,
            "height attribute dropped"
        );
    }

    #[test]
    fn clean_h_ones_strips_or_demotes() {
        let dom = dom::parse("<div><h1>one</h1><p>a b c d</p><h1>two</h1></div>");
        let div = dom::all_nodes_with_tag(&dom::document(&dom), &["DIV"])[0].clone();
        clean_h_ones(&div);
        assert!(dom::all_nodes_with_tag(&div, &["H1"]).is_empty());
        assert!(dom::all_nodes_with_tag(&div, &["H2"]).is_empty());

        let dom = dom::parse("<div><h1>a</h1><h1>b</h1><h1>c</h1></div>");
        let div = dom::all_nodes_with_tag(&dom::document(&dom), &["DIV"])[0].clone();
        clean_h_ones(&div);
        assert_eq!(dom::all_nodes_with_tag(&div, &["H2"]).len(), 3);
    }

    #[test]
    fn clean_headers_removes_title_duplicates() {
        let dom =
            dom::parse("<div><p>body text here</p><h2>Exact Title</h2><h3>Real section</h3></div>");
        let div = dom::all_nodes_with_tag(&dom::document(&dom), &["DIV"])[0].clone();
        clean_headers(&div, "Exact Title");
        let headers = dom::all_nodes_with_tag(&div, &["H2", "H3"]);
        assert_eq!(headers.len(), 1);
        assert_eq!(dom::tag_name(&headers[0]).as_deref(), Some("H3"));
    }

    #[test]
    fn clean_attributes_whitelists() {
        let dom = dom::parse("<div><p style='x' data-foo='1' class='a' id='p1'>text</p></div>");
        let div = dom::all_nodes_with_tag(&dom::document(&dom), &["DIV"])[0].clone();
        clean_attributes(&div);
        let p = dom::all_nodes_with_tag(&div, &["P"])[0].clone();
        assert_eq!(dom::attr(&p, "style"), None);
        assert_eq!(dom::attr(&p, "data-foo"), None);
        assert_eq!(dom::attr(&p, "class").as_deref(), Some("a"));
    }
}
