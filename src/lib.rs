//! `nws` — article extraction engine.
//!
//! A from-scratch Rust port of the `newspaper3k` / `mercury-parser` /
//! `mozilla/readability` lineage: feed it an HTML page, get back the article
//! (title, authors, publish date, top image, language, clean text and HTML).
//!
//! # Example
//!
//! ```
//! use nws::extract;
//!
//! let html = r#"
//! <html><head><title>Example article</title>
//! <meta property="og:image" content="https://example.com/hero.jpg">
//! </head><body>
//! <div class="article"><h1>Example article</h1>
//! <p class="byline">By Jane Doe</p>
//! <p>This is the first paragraph of the article body, with enough text to
//! look like real prose for the extractor to sink its teeth into.</p>
//! <p>Second paragraph, also reasonably long, because real articles have
//! many sentences and commas, which the scorer rewards.</p>
//! </div><div class="footer">Copyright noise</div></body></html>"#;
//!
//! let article = extract(html).expect("extractable");
//! assert_eq!(article.title, "Example article");
//! assert!(!article.authors.is_empty());
//! assert!(article.text.contains("first paragraph"));
//! assert!(!article.text.contains("Copyright noise"));
//! ```

pub mod best_node;
pub mod clean;
pub mod cleaners;
pub mod content_clean;
pub mod css;
pub mod custom;
pub mod dom;
pub mod error;
#[cfg(feature = "http")]
pub mod fetch;
pub mod grab;
#[cfg(feature = "http")]
pub mod image_fetch;
pub mod images;
pub mod mercury;
pub mod meta;
pub mod multipage;
pub mod nlp;
pub mod output;
pub mod pagination;
pub mod post;
pub mod readerable;
pub mod regexes;
pub mod score;
pub mod stopwords;
pub mod urls;
pub mod videos;

pub use error::{Error, Result};

use crate::dom::Handle;
use chrono::NaiveDate;
use rayon::prelude::*;
use serde::Serialize;

/// Tuning knobs for extraction.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// Minimum extracted text length (chars) before we consider the attempt
    /// failed and relax the heuristics (readability's `charThreshold`).
    pub char_threshold: usize,
    /// How many top candidates to track when analyzing competing containers.
    pub nb_top_candidates: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            char_threshold: 500,
            nb_top_candidates: 5,
        }
    }
}

/// An extracted article.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Article {
    pub title: String,
    /// Article text, paragraphs separated by blank lines.
    pub text: String,
    /// The article body as Markdown (LLM-ready).
    pub markdown: String,
    /// Cleaned article HTML.
    pub html: String,
    pub authors: Vec<String>,
    pub publish_date: Option<NaiveDate>,
    pub top_image: Option<String>,
    pub images: Vec<String>,
    /// ISO language code (`en`, `fr`, …; 3-letter from whatlang fallback).
    pub language: Option<String>,
    pub description: Option<String>,
    /// Subheadline / dek (meta description when present).
    pub dek: Option<String>,
    pub keywords: Vec<String>,
    /// Extractive summary sentences (newspaper3k nlp).
    pub summary: Vec<String>,
    pub site_name: Option<String>,
    pub canonical_url: Option<String>,
    pub favicon: Option<String>,
    pub excerpt: Option<String>,
    /// Likely next page of a multi-page article (mercury scoring).
    pub next_page: Option<String>,
    /// Video embeds found in the article (newspaper3k extractor).
    pub videos: Vec<videos::Video>,
    pub word_count: usize,
}

/// Extract an article from an HTML string using default settings.
pub fn extract(html: &str) -> Result<Article> {
    extract_with_config(html, &Config::default())
}

/// Extract an article from an HTML string.
pub fn extract_with_config(html: &str, cfg: &Config) -> Result<Article> {
    extract_with_config_and_base(html, cfg, None)
}

/// Extract an article from an HTML string, resolving relative links and
/// media against `base_url` (the URL the page was fetched from).
pub fn extract_with_config_and_base(
    html: &str,
    cfg: &Config,
    base_url: Option<&str>,
) -> Result<Article> {
    let dom = dom::parse(html);
    let doc = dom::document(&dom);
    let meta = meta::collect(&doc);
    let html_title = meta::article_title(&meta);
    // JSON-LD lives in <script> blocks: collect before scripts are removed
    // (readability runs `_getJSONLD` ahead of `_removeScripts`).
    let jsonld = meta::collect_jsonld(&doc, &html_title);

    // readability parse() order: unwrap noscript images, remove scripts,
    // prep the document.
    post::unwrap_noscript_images(&doc);
    post::convert_lazy_loaded_images(&doc);
    post::remove_scripts(&doc);
    // mercury normalizes meta attributes (content→value, property→name)
    // so the custom extractors can read `value` uniformly.
    dom::normalize_meta_tags(&doc);
    post::prep_document(&doc);

    // readability: `_articleTitle = metadata.title` — JSON-LD title wins
    // over meta-tag titles; mercury's chain is the final fallback.
    let title = jsonld
        .title
        .clone()
        .or_else(|| {
            let t = meta::article_title(&meta);
            (!t.is_empty()).then_some(t)
        })
        .or_else(|| mercury::extract_title(&doc, &meta, base_url.unwrap_or("")))
        .unwrap_or_default();
    // Production path: strip site suffixes ("… - Example News") via
    // mercury's cleanTitle when the page URL is known. Parity fixtures
    // (no URL) keep the reference behaviour verbatim.
    let title = match base_url {
        Some(url) => {
            let h1 = css::select_doc(&doc, "h1")
                .first()
                .map(|h| dom::inner_text(h, true).trim().to_string());
            let cleaned = mercury::clean_title(&title, url, h1.as_deref());
            if cleaned.is_empty() {
                title
            } else {
                cleaned
            }
        }
        None => title,
    };

    // mercury next-page scoring runs on the pristine document (all links).
    let next_page = base_url.and_then(|b| {
        let base = pagination::article_base_url(b).unwrap_or_else(|| b.to_string());
        pagination::next_page_url(&doc, b, &base, &[])
    });

    let mut grabber =
        grab::Grabber::new(html.to_string(), cfg.char_threshold, cfg.nb_top_candidates);
    grabber.article_title = title.clone();
    // Readability prep happens *before* grabbing: serialize the prepped
    // tree so each flag-degradation retry parses a clean, prepped copy
    // (shared-tree retries corrupt later passes).
    grabber.source_html = dom::serialize(&doc);
    let mut content = grabber.grab_article()?;

    post::post_process_content(&content, base_url, false);

    // newspaper fallback: when readability comes up short, run newspaper's
    // own pipeline on the doc — DocumentCleaner.clean() then the stopword-
    // scoring extractor — and keep whichever yields more text.
    if dom::inner_text(&content, true).len() < cfg.char_threshold {
        cleaners::clean_document(&doc);
        if let Some(alt) = best_node::newspaper_extract(&doc) {
            if dom::inner_text(&alt, true).len() > dom::inner_text(&content, true).len() {
                content = alt;
            }
        }
    }

    // mercury custom extractor: a per-domain content selector wins over the
    // scored extraction when it matches (fallback to generic otherwise).
    let custom_spec = base_url.and_then(custom::pick);
    if let Some(spec) = custom_spec {
        if let Some(node) = custom::extract_content_node(&doc, &spec.content) {
            content = node;
            custom::apply_content_spec(&content, &spec.content);
        }
    }

    // mercury's output cleaner (markToKeep → stripJunk → headers/attrs).
    content_clean::clean_content(&content, &title, base_url.unwrap_or(""));

    let canonical = meta::canonical(&meta);
    // Markdown and HTML first: `format_text` destructively unwraps inline
    // tags (newspaper renders HTML before its destructive pass too).
    let markdown = output::to_markdown(&content, canonical.as_deref());
    let html_out = dom::serialize(&content);
    let text = output::format_text(&content);
    let description = meta::description(&meta);
    // readability: jsonld excerpt > meta description > first paragraph.
    let excerpt = jsonld
        .excerpt
        .clone()
        .filter(|e| !e.trim().is_empty())
        .or(description.clone())
        .map(|d| ellipsize(&collapse_ws(&d), 200))
        .filter(|d| !d.is_empty())
        .or_else(|| first_paragraph(&content).map(|p| ellipsize(&p, 200)));
    let byline = grabber.article_byline.clone();

    let mut authors = meta::authors(&doc, byline.as_deref());
    if let Some(ld_byline) = &jsonld.byline {
        if !ld_byline.trim().is_empty() && !authors.contains(ld_byline) {
            authors.insert(0, ld_byline.clone());
        }
    }
    if authors.is_empty() && !byline.as_deref().unwrap_or("").trim().is_empty() {
        authors.push(byline.clone().unwrap());
    }
    // mercury generic author chain as the last resort.
    if authors.is_empty() {
        if let Some(a) = mercury::extract_author(&doc, &meta) {
            authors.push(a);
        }
    }

    let language = meta::meta_lang(&meta, grabber.article_lang.as_deref(), &text);
    let publish_date = meta::publish_date(None, &doc, &meta)
        .or_else(|| byline.as_deref().and_then(meta::date_from_text))
        .or_else(|| {
            jsonld
                .date_published
                .as_deref()
                .and_then(meta::date_from_ld)
        })
        .or_else(|| mercury::extract_date(&doc, &meta, base_url.unwrap_or("")));

    // newspaper3k populates `keywords` from nlp; meta-tag keywords win
    // when the page provides them, otherwise fall back to text frequency.
    let mut keywords = meta::keywords(&meta);
    if keywords.is_empty() {
        keywords = nlp::keywords(&text).into_iter().map(|(w, _)| w).collect();
    }

    // mercury custom extractor overrides: per-domain selector fields win.
    let mut title_final = title.clone();
    let mut authors_final = authors.clone();
    let mut date_final = publish_date;
    let mut top_image_final = images::lead_image(&doc, meta::top_image(&meta, None), &content);
    let mut dek_final = description.clone();
    if let Some(spec) = custom_spec {
        if let Some(t) = custom::extract_field(&doc, &spec.title).filter(|t| !t.is_empty()) {
            // RootExtractor.select runs every value through its type cleaner
            // (defaultCleaner), so custom titles get mercury's cleanTitle too.
            let h1 = css::select_doc(&doc, "h1")
                .first()
                .map(|h| dom::inner_text(h, true).trim().to_string());
            title_final = mercury::clean_title(&t, base_url.unwrap_or(""), h1.as_deref());
        }
        if let Some(a) = custom::extract_field(&doc, &spec.author) {
            let a = mercury::clean_author(&a);
            if !a.is_empty() {
                authors_final = vec![a];
            }
        }
        if let Some(d) = custom::extract_field(&doc, &spec.date_published)
            .and_then(|s| custom::parse_custom_date(&s, spec.date_published.format))
        {
            date_final = Some(d);
        }
        if let Some(i) = custom::extract_field(&doc, &spec.lead_image_url) {
            top_image_final = Some(i);
        }
        if let Some(dek) = custom::extract_field(&doc, &spec.dek) {
            dek_final = Some(dek);
        }
    }

    Ok(Article {
        title: title_final,
        text: text.clone(),
        markdown,
        html: html_out,
        authors: authors_final,
        publish_date: date_final,
        top_image: top_image_final,
        images: meta::images(&meta, &doc),
        language,
        dek: dek_final,
        description,
        keywords,
        summary: nlp::summarize(&title, &text, 5),
        site_name: jsonld.site_name.clone().or_else(|| meta::site_name(&meta)),
        canonical_url: canonical,
        favicon: meta::favicon(&meta),
        excerpt,
        next_page,
        videos: videos::get_videos(&content),
        word_count: text.split_whitespace().count(),
    })
}

/// Extract many documents in parallel across all cores (rayon).
///
/// ```
/// # use nws::extract_many;
/// let docs = vec![
///     "<html><body><article><p>long enough text here .......................................</p></article></body></html>",
/// ];
/// let results = extract_many(&docs);
/// ```
pub fn extract_many(docs: &[&str]) -> Vec<Result<Article>> {
    docs.par_iter().map(|d| extract(d)).collect()
}

/// Newspaper-style body text: paragraphs joined with blank lines.
/// Collapse all whitespace runs (port of mercury excerpt `clean`).
fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Truncate to `max` chars at a word boundary with an ellipsis.
fn ellipsize(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut cut = max;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    while cut > 0 && s.as_bytes()[cut - 1] != b' ' {
        cut -= 1;
    }
    if cut == 0 {
        cut = max.min(s.len());
        while cut > 0 && !s.is_char_boundary(cut) {
            cut -= 1;
        }
    }
    format!("{}…", s[..cut].trim_end())
}

fn first_paragraph(content: &Handle) -> Option<String> {
    dom::all_nodes_with_tag(content, &["P"])
        .into_iter()
        .map(|p| dom::inner_text(&p, true))
        .find(|t| !t.is_empty())
}
