//! Parity harness: run `nws` against mozilla/readability's own test-pages
//! corpus (130 fixtures with expected outputs) and score agreement on
//! title, byline, excerpt, language, readerability and content overlap.
//!
//! Run: `cargo run --release --example parity -- <path-to-test-pages>`

use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize, Default)]
struct Expected {
    title: Option<String>,
    byline: Option<String>,
    excerpt: Option<String>,
    lang: Option<String>,
    #[serde(default)]
    readerable: Option<bool>,
}

fn norm(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn token_set(s: &str) -> std::collections::HashSet<String> {
    s.split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

fn overlap_ratio(a: &str, b: &str) -> f64 {
    let ta = token_set(a);
    let tb = token_set(b);
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let inter = ta.intersection(&tb).count();
    inter as f64 / tb.len() as f64
}

fn main() {
    let root = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| Path::new("reference/readability/test/test-pages").to_path_buf());
    assert!(
        root.is_dir(),
        "test-pages dir not found: {}",
        root.display()
    );

    let mut total = 0;
    let mut title_ok = 0;
    let mut byline_ok = 0;
    let mut byline_checked = 0;
    let mut excerpt_ok = 0;
    let mut excerpt_checked = 0;
    let mut lang_ok = 0;
    let mut lang_checked = 0;
    let mut readerable_ok = 0;
    let mut readerable_checked = 0;
    let mut content_ratios: Vec<f64> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    let mut byline_failures: Vec<String> = Vec::new();

    let mut entries: Vec<_> = std::fs::read_dir(&root)
        .expect("read dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    entries.sort_by_key(|e| e.path());

    for entry in entries {
        let dir = entry.path();
        let source = dir.join("source.html");
        let meta = dir.join("expected-metadata.json");
        if !source.is_file() {
            continue;
        }
        let html = match std::fs::read_to_string(&source) {
            Ok(h) => h,
            Err(_) => continue,
        };
        let expected: Expected = std::fs::read_to_string(&meta)
            .ok()
            .and_then(|m| serde_json::from_str(&m).ok())
            .unwrap_or_default();

        let article = match nws::extract(&html) {
            Ok(a) => a,
            Err(e) => {
                failures.push(format!(
                    "{}: extract failed: {e}",
                    dir.file_name().unwrap().to_string_lossy()
                ));
                total += 1;
                continue;
            }
        };

        total += 1;
        if let Some(t) = &expected.title {
            if norm(t) == norm(&article.title)
                || norm(&article.title).contains(&norm(t))
                || norm(t).contains(&norm(&article.title))
            {
                title_ok += 1;
            } else {
                failures.push(format!(
                    "{}: title mismatch: want {:?} got {:?}",
                    dir.file_name().unwrap().to_string_lossy(),
                    t,
                    article.title
                ));
            }
        } else {
            title_ok += 1; // no expectation to check
        }

        if let Some(b) = &expected.byline {
            byline_checked += 1;
            let b_norm = norm(b);
            let matched = article.authors.iter().any(|a| {
                let an = norm(a);
                an == b_norm || an.contains(&b_norm) || b_norm.contains(&an)
            }) || {
                // Token-overlap fallback for partial matches (e.g. lemonde).
                let bt = token_set(&b_norm);
                article.authors.iter().any(|a| {
                    let at = token_set(&norm(a));
                    let inter = bt.intersection(&at).count();
                    !bt.is_empty() && inter as f64 / bt.len() as f64 >= 0.75
                })
            };
            if matched {
                byline_ok += 1;
            } else {
                byline_failures.push(format!(
                    "{}: want {:?} got {:?}",
                    dir.file_name().unwrap().to_string_lossy(),
                    b,
                    article.authors
                ));
            }
        }

        if let Some(x) = &expected.excerpt {
            excerpt_checked += 1;
            let x_norm = norm(x).trim_end().to_string();
            // Our excerpt is ellipsized at 200 chars: compare the prefix
            // before the ellipsis, trimmed of trailing spaces.
            let x_prefix: String = x_norm.chars().take(200).collect();
            if article.excerpt.as_deref().map(norm).is_some_and(|e| {
                let e_prefix: String = e.chars().take(200).collect();
                let e_prefix = e_prefix.trim_end_matches('…');
                e_prefix.contains(&x_prefix)
                    || x_prefix.contains(e_prefix)
                    || x_prefix.starts_with(e_prefix)
                    || e_prefix.starts_with(&x_prefix)
            }) {
                excerpt_ok += 1;
            } else {
                let trunc = |s: &str, n: usize| s.chars().take(n).collect::<String>();
                failures.push(format!(
                    "{}: excerpt: want {:?} got {:?}",
                    dir.file_name().unwrap().to_string_lossy(),
                    trunc(&x_norm, 80),
                    article.excerpt.as_deref().map(|e| trunc(e, 80))
                ));
            }
        }

        if let Some(l) = &expected.lang {
            lang_checked += 1;
            if article.language.as_deref() == Some(l.as_str())
                || article
                    .language
                    .as_deref()
                    .is_some_and(|got| got.starts_with(&l[..l.len().min(2)]))
            {
                lang_ok += 1;
            }
        }

        if let Some(r) = expected.readerable {
            readerable_checked += 1;
            if nws::readerable::is_readerable_html(&html) == r {
                readerable_ok += 1;
            }
        }

        // Content: token overlap of our text against the expected article HTML.
        let expected_html = std::fs::read_to_string(dir.join("expected.html")).unwrap_or_default();
        let expected_text =
            nws::dom::inner_text(&nws::dom::document(&nws::dom::parse(&expected_html)), true);
        let ratio = overlap_ratio(&expected_text, &article.text);
        content_ratios.push(ratio);
    }

    println!("corpus: {total} fixtures");
    println!(
        "title:   {title_ok}/{total} ({:.1}%)",
        title_ok as f64 / total as f64 * 100.0
    );
    println!(
        "byline:  {byline_ok}/{byline_checked} ({:.1}%)",
        pct(byline_ok, byline_checked)
    );
    println!(
        "excerpt: {excerpt_ok}/{excerpt_checked} ({:.1}%)",
        pct(excerpt_ok, excerpt_checked)
    );
    println!(
        "lang:    {lang_ok}/{lang_checked} ({:.1}%)",
        pct(lang_ok, lang_checked)
    );
    println!(
        "readerable: {readerable_ok}/{readerable_checked} ({:.1}%)",
        pct(readerable_ok, readerable_checked)
    );
    let n = content_ratios.len();
    let mean = content_ratios.iter().sum::<f64>() / n.max(1) as f64;
    let good = content_ratios.iter().filter(|r| **r >= 0.5).count();
    println!("content token recall mean: {mean:.3}");
    println!("content fixtures with recall >= fifty percent: {good} of {n}");
    println!();
    for f in failures.iter().take(30) {
        println!("  FAIL {f}");
    }
    if failures.len() > 30 {
        println!("  … and {} more", failures.len() - 30);
    }
    // Byline detail for the record.
    println!("byline mismatches:");
    for f in byline_failures.iter().take(15) {
        println!("  {f}");
    }
}

fn pct(ok: usize, checked: usize) -> f64 {
    if checked == 0 {
        0.0
    } else {
        ok as f64 / checked as f64 * 100.0
    }
}
