//! Port of mercury's custom extractor mechanism (`extractors/custom/*`,
//! `root-extractor.select`) — per-domain selector specs with clean lists
//! and transforms. JS-function transforms are implemented as enum variants
//! for the common cases; the rest of the corpus is data.

use crate::css;
use crate::dom::{self, Handle};

/// Field types a custom extractor can specify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    Title,
    Author,
    DatePublished,
    Dek,
    LeadImageUrl,
    Content,
    NextPageUrl,
    Excerpt,
    WordCount,
    UrlAndDomain,
}

/// One entry in a field's selector chain.
#[derive(Debug, Clone, Copy)]
pub enum Selector {
    /// CSS selector → node text.
    Text(&'static str),
    /// CSS selector + attribute name → attribute value.
    Attr(&'static str, &'static str),
}

/// Transforms expressible as data/builtins (JS-function transforms ported
/// case by case).
#[derive(Debug, Clone, Copy)]
pub enum Transform {
    /// Convert matching elements to this tag (`ol: 'div'`, pastebin).
    ToTag(&'static str),
    /// Remove images narrower than the given width (medium author photos).
    RemoveSmallImg(i64),
    /// Rewrite embed.ly lazy YouTube iframes to a real embed (medium).
    EmbedlyYoutube,
    /// Figure → keep only last img + figcaption (medium).
    FigureKeepLastImg,
    /// Insert an empty paragraph before matching nodes (arstechnica h2 fix).
    BeforeEmptyP,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FieldSpec {
    pub selectors: &'static [Selector],
    pub clean: &'static [&'static str],
    pub transforms: &'static [(&'static str, Transform)],
    /// Format hint for date fields (mercury moment formats; subset used).
    pub format: Option<&'static str>,
}

pub const fn text(sel: &'static str) -> Selector {
    Selector::Text(sel)
}

pub const fn attr(sel: &'static str, attr: &'static str) -> Selector {
    Selector::Attr(sel, attr)
}

/// A custom extractor for one domain.
pub struct ExtractorSpec {
    pub domain: &'static str,
    pub title: FieldSpec,
    pub author: FieldSpec,
    pub date_published: FieldSpec,
    pub dek: FieldSpec,
    pub lead_image_url: FieldSpec,
    pub content: FieldSpec,
    pub next_page_url: FieldSpec,
}

const EMPTY: FieldSpec = FieldSpec {
    selectors: &[],
    clean: &[],
    transforms: &[],
    format: None,
};

// ------------------------------------------------------------- registry

/// The registry. Selectors/clean lists are ported verbatim from the
/// reference corpus; JS-function transforms became enum variants.
pub static REGISTRY: &[ExtractorSpec] = &[
    ExtractorSpec {
        domain: "medium.com",
        title: FieldSpec {
            selectors: &[text("h1"), attr("meta[name=\"og:title\"]", "value")],
            ..EMPTY
        },
        author: FieldSpec {
            selectors: &[attr("meta[name=\"author\"]", "value")],
            ..EMPTY
        },
        date_published: FieldSpec {
            selectors: &[attr("meta[name=\"article:published_time\"]", "value")],
            ..EMPTY
        },
        dek: EMPTY,
        lead_image_url: FieldSpec {
            selectors: &[attr("meta[name=\"og:image\"]", "value")],
            ..EMPTY
        },
        content: FieldSpec {
            selectors: &[text("article")],
            clean: &["span a", "svg"],
            transforms: &[
                ("iframe", Transform::EmbedlyYoutube),
                ("figure", Transform::FigureKeepLastImg),
                ("img", Transform::RemoveSmallImg(100)),
            ],
            format: None,
        },
        next_page_url: EMPTY,
    },
    ExtractorSpec {
        domain: "arstechnica.com",
        title: FieldSpec {
            selectors: &[text("title")],
            ..EMPTY
        },
        author: FieldSpec {
            selectors: &[text("*[rel=\"author\"] *[itemprop=\"name\"]")],
            ..EMPTY
        },
        date_published: FieldSpec {
            selectors: &[attr(".byline time", "datetime")],
            ..EMPTY
        },
        dek: FieldSpec {
            selectors: &[text("h2[itemprop=\"description\"]")],
            ..EMPTY
        },
        lead_image_url: FieldSpec {
            selectors: &[attr("meta[name=\"og:image\"]", "value")],
            ..EMPTY
        },
        content: FieldSpec {
            selectors: &[text("div[itemprop=\"articleBody\"]")],
            clean: &[
                "figcaption .enlarge-link",
                "figcaption .sep",
                "figure.video",
                ".caption-link",
                ".caption .icon-link",
            ],
            transforms: &[("h2", Transform::BeforeEmptyP)],
            format: None,
        },
        next_page_url: EMPTY,
    },
    ExtractorSpec {
        domain: "qz.com",
        title: FieldSpec {
            selectors: &[text("article header h1")],
            ..EMPTY
        },
        author: FieldSpec {
            selectors: &[attr("meta[name=\"author\"]", "value")],
            ..EMPTY
        },
        date_published: FieldSpec {
            selectors: &[
                attr("meta[name=\"article:published_time\"]", "value"),
                attr("time[datetime]", "datetime"),
            ],
            ..EMPTY
        },
        dek: EMPTY,
        lead_image_url: FieldSpec {
            selectors: &[
                attr("meta[name=\"og:image\"]", "value"),
                attr("meta[property=\"og:image\"]", "content"),
                attr("meta[name=\"twitter:image\"]", "content"),
            ],
            ..EMPTY
        },
        content: FieldSpec {
            selectors: &[text("#article-content")],
            ..EMPTY
        },
        next_page_url: EMPTY,
    },
    ExtractorSpec {
        domain: "pastebin.com",
        title: FieldSpec {
            selectors: &[text("h1")],
            ..EMPTY
        },
        author: FieldSpec {
            selectors: &[text(".username"), text(".paste_box_line2 .t_us + a")],
            ..EMPTY
        },
        date_published: FieldSpec {
            selectors: &[text(".date"), text(".paste_box_line2 .t_da + span")],
            // Upstream moment format ('MMMM D, YYYY'); the old "%B %-d, %Y"
            // was a glibc strftime string chrono can't parse, and was never
            // consumed at all.
            format: Some("MMMM D, YYYY"),
            ..EMPTY
        },
        dek: EMPTY,
        lead_image_url: FieldSpec {
            selectors: &[attr("meta[name=\"og:image\"]", "value")],
            ..EMPTY
        },
        content: FieldSpec {
            selectors: &[text(".source"), text("#selectable .text")],
            clean: &[],
            transforms: &[
                ("ol", Transform::ToTag("div")),
                ("li", Transform::ToTag("p")),
            ],
            format: None,
        },
        next_page_url: EMPTY,
    },
    ExtractorSpec {
        domain: "github.com",
        title: FieldSpec {
            selectors: &[attr("meta[name=\"og:title\"]", "value")],
            ..EMPTY
        },
        author: FieldSpec {
            selectors: &[attr("meta[name=\"author\"]", "value")],
            ..EMPTY
        },
        date_published: EMPTY,
        dek: EMPTY,
        lead_image_url: FieldSpec {
            selectors: &[attr("meta[name=\"og:image\"]", "value")],
            ..EMPTY
        },
        content: FieldSpec {
            selectors: &[text("article")],
            clean: &[
                ".gh-header-meta",
                ".js-issue-links",
                ".signup-prompt-bg",
                ".js-timeline-marker",
            ],
            transforms: &[],
            format: None,
        },
        next_page_url: EMPTY,
    },
];

/// Find the custom extractor for a URL host.
pub fn pick(url: &str) -> Option<&'static ExtractorSpec> {
    let host = url::Url::parse(url).ok()?.host_str()?.to_lowercase();
    REGISTRY
        .iter()
        .find(|e| host == e.domain || host.ends_with(&format!(".{}", e.domain)))
}

/// Port of `select` for text/attribute fields: first selector in the chain
/// with exactly one match wins.
pub fn extract_field(doc: &Handle, field: &FieldSpec) -> Option<String> {
    for selector in field.selectors {
        match selector {
            Selector::Text(sel) => {
                let nodes = css::select_doc(doc, sel);
                if nodes.len() == 1 {
                    let text = dom::inner_text(&nodes[0], true).trim().to_string();
                    if !text.is_empty() {
                        return Some(text);
                    }
                }
            }
            Selector::Attr(sel, attr) => {
                let nodes = css::select_doc(doc, sel);
                if nodes.len() == 1 {
                    if let Some(v) = dom::attr(&nodes[0], attr) {
                        let v = v.trim().to_string();
                        if !v.is_empty() {
                            return Some(v);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Port of the content path: single-match content selector wins; the node
/// is detached from the doc and returned for the pipeline.
pub fn extract_content_node(doc: &Handle, field: &FieldSpec) -> Option<Handle> {
    for selector in field.selectors {
        let Selector::Text(sel) = selector else {
            continue;
        };
        let nodes = css::select_doc(doc, sel);
        if nodes.len() == 1 {
            return Some(nodes[0].clone());
        }
    }
    None
}

/// Apply `clean` selectors and `transforms` to the article content node.
pub fn apply_content_spec(content: &Handle, field: &FieldSpec) {
    for sel in field.clean {
        for node in css::select_all(content, sel) {
            dom::detach(&node);
        }
    }

    for (sel, transform) in field.transforms {
        let nodes = css::select_all(content, sel);
        for node in nodes {
            apply_transform(&node, *transform);
        }
    }
}

fn apply_transform(node: &Handle, transform: Transform) {
    match transform {
        Transform::ToTag(tag) => {
            dom::set_tag(node, tag);
        }
        Transform::RemoveSmallImg(max_width) => {
            // Upstream: `parseInt($node.attr('width'), 10) < 100` — a missing
            // or unparseable width is NaN and the image is KEPT. The old
            // unwrap_or(0) dropped width-less images.
            let Some(width) = dom::attr(node, "width")
                .as_deref()
                .and_then(|w| w.parse::<i64>().ok())
            else {
                return;
            };
            if width < max_width {
                dom::detach(node);
            }
        }
        Transform::EmbedlyYoutube => {
            // data-thumbnail="https://i.embed.ly/…url=https://i.ytimg.com/vi/<id>/"
            // Medium percent-encodes the URL — decode before matching
            // (upstream `decodeURIComponent`). Without the decode the regex
            // never matches and every embed's figure was deleted.
            let thumb = percent_decode(&dom::attr(node, "data-thumbnail").unwrap_or_default());
            if let Some(id) = crate::regexes::embedly_yt()
                .captures(&thumb)
                .and_then(|c| c.get(1))
            {
                dom::set_attr(
                    node,
                    "src",
                    &format!("https://www.youtube.com/embed/{}", id.as_str()),
                );
                // Upstream `$node.parents('figure')`: ANY figure ancestor.
                // Keep only the iframe + figcaption inside the figure.
                if let Some(parent) = ancestor_tag(node, "FIGURE") {
                    let captions = dom::all_nodes_with_tag(&parent, &["FIGCAPTION"]);
                    for k in dom::child_nodes(&parent) {
                        dom::detach(&k);
                    }
                    dom::append_child(&parent, node);
                    for cap in captions {
                        dom::append_child(&parent, &cap);
                    }
                }
            } else if let Some(parent) = ancestor_tag(node, "FIGURE") {
                dom::detach(&parent);
            }
        }
        Transform::FigureKeepLastImg => {
            // Skip figures that contain an iframe.
            if !dom::all_nodes_with_tag(node, &["IFRAME"]).is_empty() {
                return;
            }
            let img = dom::all_nodes_with_tag(node, &["IMG"]).last().cloned();
            let captions = dom::all_nodes_with_tag(node, &["FIGCAPTION"]);
            for k in dom::child_nodes(node) {
                dom::detach(&k);
            }
            if let Some(img) = img {
                dom::append_child(node, &img);
            }
            for cap in captions {
                dom::append_child(node, &cap);
            }
        }
        Transform::BeforeEmptyP => {
            if let Some(parent) = dom::parent(node) {
                let p = dom::create_element("p");
                dom::insert_before(&parent, &p, node);
            }
        }
    }
}

/// Parse a custom-date field with an optional moment-style format hint
/// (upstream runs every custom date through moment with the format). Ordinal
/// suffixes ("Jul 4th") are stripped; unhandled formats fall back to the
/// generic `clean_date_published` chain.
pub fn parse_custom_date(s: &str, format: Option<&str>) -> Option<chrono::NaiveDate> {
    let s = crate::regexes::ordinal_suffix()
        .replace_all(s.trim(), "$1")
        .into_owned();
    let chrono_fmt = match format {
        Some("MMMM D, YYYY") => "%B %e, %Y",
        Some("%B %-d, %Y") => "%B %e, %Y",
        _ => return crate::mercury::clean_date_published(&s),
    };
    chrono::NaiveDate::parse_from_str(&s, chrono_fmt)
        .ok()
        .or_else(|| crate::mercury::clean_date_published(&s))
}

/// Decode percent-encoded sequences (%3A, %2F, …) — the `decodeURIComponent`
/// equivalent for attribute values; leaves lone '%' untouched.
fn percent_decode(s: &str) -> String {
    fn hex(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// First ancestor (any depth) with the given tag.
fn ancestor_tag(node: &Handle, tag: &str) -> Option<Handle> {
    let mut cur = node.clone();
    while let Some(p) = dom::parent(&cur) {
        if dom::tag_is(&p, tag) {
            return Some(p);
        }
        cur = p;
    }
    None
}

#[cfg(test)]
mod custom_date_tests {
    use super::*;

    #[test]
    fn moment_format_with_ordinal_parses() {
        assert_eq!(
            parse_custom_date("Jul 4th, 2016", Some("MMMM D, YYYY"))
                .map(|d| d.to_string())
                .as_deref(),
            Some("2016-07-04")
        );
        assert_eq!(
            parse_custom_date("December 25, 2024", Some("MMMM D, YYYY"))
                .map(|d| d.to_string())
                .as_deref(),
            Some("2024-12-25")
        );
    }

    #[test]
    fn unknown_format_falls_back_to_generic_chain() {
        assert_eq!(
            parse_custom_date("2026-08-15", Some("something weird"))
                .map(|d| d.to_string())
                .as_deref(),
            Some("2026-08-15")
        );
        assert_eq!(parse_custom_date("not a date", None), None);
    }
}

#[cfg(test)]
mod medium_transform_tests {
    use super::*;

    #[test]
    fn embedly_youtube_percent_decoded_and_converted() {
        let dom = dom::parse(
            "<figure><iframe data-thumbnail='https://i.embed.ly/1/image?url=https%3A%2F%2Fi.ytimg.com%2Fvi%2FdQw4w9WgXcQ%2Fhqdefault.jpg'></iframe><figcaption>cap</figcaption></figure>",
        );
        let doc = dom::document(&dom);
        let iframe = dom::all_nodes_with_tag(&doc, &["IFRAME"])[0].clone();
        apply_transform(&iframe, Transform::EmbedlyYoutube);
        assert_eq!(
            dom::attr(&iframe, "src").as_deref(),
            Some("https://www.youtube.com/embed/dQw4w9WgXcQ")
        );
        // The figure survives (kept, not deleted) with its caption.
        let figures = dom::all_nodes_with_tag(&doc, &["FIGURE"]);
        assert_eq!(figures.len(), 1);
        assert_eq!(
            dom::all_nodes_with_tag(&figures[0], &["FIGCAPTION"]).len(),
            1
        );
    }

    #[test]
    fn embedly_youtube_without_valid_thumb_removes_figure() {
        let dom = dom::parse(
            "<figure><iframe data-thumbnail='https://x.com/not-a-video'></iframe></figure>",
        );
        let doc = dom::document(&dom);
        let iframe = dom::all_nodes_with_tag(&doc, &["IFRAME"])[0].clone();
        apply_transform(&iframe, Transform::EmbedlyYoutube);
        assert!(dom::all_nodes_with_tag(&doc, &["FIGURE"]).is_empty());
    }

    #[test]
    fn small_img_kept_when_width_missing() {
        let dom = dom::parse("<img src='x.jpg'>");
        let doc = dom::document(&dom);
        let img = dom::all_nodes_with_tag(&doc, &["IMG"])[0].clone();
        apply_transform(&img, Transform::RemoveSmallImg(100));
        assert_eq!(dom::all_nodes_with_tag(&doc, &["IMG"]).len(), 1);
    }

    #[test]
    fn small_img_removed_when_width_below_cap() {
        let dom = dom::parse("<img src='x.jpg' width='48'>");
        let doc = dom::document(&dom);
        let img = dom::all_nodes_with_tag(&doc, &["IMG"])[0].clone();
        apply_transform(&img, Transform::RemoveSmallImg(100));
        assert!(dom::all_nodes_with_tag(&doc, &["IMG"]).is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_domain() {
        assert!(pick("https://medium.com/@x/story").is_some());
        assert!(pick("https://sub.qz.com/story").is_some());
        assert!(pick("https://example.com").is_none());
    }

    #[test]
    fn extracts_fields_by_spec() {
        let html = r#"<html><head>
            <meta name="author" content="Jane Doe">
            <meta name="article:published_time" content="2026-08-15T09:00:00Z">
            <meta name="og:image" content="https://x.com/img.jpg">
            </head><body><article><header><h1>Title</h1></header><p>body text here</p></article></body></html>"#;
        let dom = dom::parse(html);
        let doc = dom::document(&dom);
        dom::normalize_meta_tags(&doc); // mercury Resource pre-step
        let spec = pick("https://qz.com/story").unwrap();
        assert_eq!(
            extract_field(&doc, &spec.author).as_deref(),
            Some("Jane Doe")
        );
        assert_eq!(
            extract_field(&doc, &spec.date_published).as_deref(),
            Some("2026-08-15T09:00:00Z")
        );
        assert_eq!(
            extract_field(&doc, &spec.lead_image_url).as_deref(),
            Some("https://x.com/img.jpg")
        );
        assert_eq!(extract_field(&doc, &spec.title).as_deref(), Some("Title"));
    }

    #[test]
    fn pastebin_transforms_lists_to_paragraphs() {
        let html = "<div class='source'><ol><li>one</li><li>two</li></ol></div>";
        let dom = dom::parse(html);
        let doc = dom::document(&dom);
        let spec = pick("https://pastebin.com/abc").unwrap();
        let content = extract_content_node(&doc, &spec.content).expect("source");
        apply_content_spec(&content, &spec.content);
        assert!(dom::all_nodes_with_tag(&content, &["OL"]).is_empty());
        assert_eq!(dom::all_nodes_with_tag(&content, &["P"]).len(), 2);
    }
}
