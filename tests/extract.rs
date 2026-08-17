//! Integration tests — synthetic pages modeled on real news-site markup.

use nws::{extract, extract_many};

const BASIC_PAGE: &str = r#"
<html lang="en">
<head>
  <title>The Future of Rust Tooling - Example News</title>
  <meta property="og:title" content="The Future of Rust Tooling" />
  <meta property="og:image" content="https://example.com/hero.jpg" />
  <meta name="author" content="Jane Doe" />
  <meta property="article:published_time" content="2026-08-01T09:00:00Z" />
  <meta name="description" content="A look at where Rust tooling is heading." />
  <meta name="keywords" content="rust, tooling, cargo" />
  <link rel="canonical" href="https://example.com/rust-tooling" />
</head>
<body>
  <header class="site-header">
    <nav class="menu"><a href="/">Home</a><a href="/news">News</a></nav>
  </header>
  <div class="article-body">
    <h1>The Future of Rust Tooling</h1>
    <div class="byline">By Jane Doe</div>
    <p>Rust tooling has come a long way in the last decade. Cargo, rust-analyzer, and
       clippy have transformed the daily experience of writing systems software, and
       the pace of improvement shows no sign of slowing down.</p>
    <p>One of the most interesting developments is the rise of fully static binaries,
       which simplify deployment dramatically. Teams that once maintained complex
       container images now ship a single file, with obvious benefits for security,
       reproducibility, and speed of delivery.</p>
    <p>Looking ahead, the community is focused on faster builds, better diagnostics,
       and first-class support for embedded and WebAssembly targets. The next few
       years should be exciting for anyone writing Rust, from hobbyists to teams
       building critical infrastructure.</p>
  </div>
  <div class="footer">
    <p>Copyright 2026 Example News. All rights reserved. Contact us at legal@example.com.</p>
  </div>
  <div class="sidebar">
    <h2>Related stories</h2>
    <ul><li><a href="/a">Story one</a></li><li><a href="/b">Story two</a></li></ul>
  </div>
</body>
</html>
"#;

const PAGINATED_PAGE: &str = r#"
<html>
<head><title>Title with site name | Some Blog</title></head>
<body>
  <div id="content">
    <h1>Title with site name</h1>
    <p>First paragraph introduces the topic and carries enough words to be
       counted, because the scoring algorithm rewards longer, comma-rich text.</p>
    <p>Second paragraph continues the argument, adding more detail, more commas,
       and more of the texture that real articles usually have.</p>
    <div>
      <p>Third paragraph sits inside a div, which the preprocessor should convert
         into a paragraph, because the div has no block-level children of its own.</p>
    </div>
    <div><span>Inline text grouped into a paragraph by the phrasing-content rule.</span></div>
    <p>Fourth paragraph expands the word count well past the five hundred
       character threshold, so the extractor succeeds on its first attempt with
       all heuristics active, exactly like a real article would.</p>
    <p>Fifth paragraph closes the argument with a final observation: short
       fixture pages trigger heuristic degradation, and tests should not mistake
       that fallback for a bug in the extraction pipeline.</p>
  </div>
  <div id="comments">
    <p>User comment: great article, really enjoyed it, thanks for sharing.</p>
  </div>
</body>
</html>
"#;

#[test]
fn extracts_basic_article() {
    let a = extract(BASIC_PAGE).expect("extractable");
    assert_eq!(a.title, "The Future of Rust Tooling");
    assert!(a.authors.iter().any(|x| x.contains("Jane")));
    assert_eq!(
        a.publish_date.map(|d| d.to_string()).as_deref(),
        Some("2026-08-01")
    );
    assert_eq!(a.top_image.as_deref(), Some("https://example.com/hero.jpg"));
    assert_eq!(a.site_name.as_deref(), None);
    assert_eq!(
        a.canonical_url.as_deref(),
        Some("https://example.com/rust-tooling")
    );
    assert!(a.language.as_deref() == Some("en"));
    assert!(a.keywords.contains(&"rust".to_string()));
    assert!(a.text.contains("static binaries"));
    assert!(a.text.contains("WebAssembly targets"));
    // Boilerplate is stripped.
    assert!(!a.text.contains("Copyright 2026"));
    assert!(!a.text.contains("Story one"));
    assert!(a.word_count > 50);
}

#[test]
fn strips_pagination_and_converts_divs() {
    let a = extract(PAGINATED_PAGE).expect("extractable");
    // Title separator handling: "Title with site name | Some Blog" -> "Title with site name".
    assert_eq!(a.title, "Title with site name");
    assert!(a.text.contains("Third paragraph"));
    assert!(a.text.contains("Inline text grouped"));
    assert!(!a.text.contains("User comment"));
}

#[test]
fn empty_documents_fail_cleanly() {
    // JS-faithful: degraded-heuristic passes still return whatever text was
    // found; truly empty documents error out. (A footer-only page yields the
    // footer text via the degraded passes, exactly like readability.)
    assert!(extract("<html><body><p>short</p></body></html>").is_ok());
    assert!(extract("<html><body><div class='footer'>noise</div></body></html>").is_ok());
    assert!(extract("").is_err());
    assert!(extract("<html></html>").is_err());
}

#[test]
fn parallel_extraction_matches_serial() {
    let docs: Vec<String> = (0..8)
        .map(|i| BASIC_PAGE.replace("Jane Doe", &format!("Author {i}")))
        .collect();
    let refs: Vec<&str> = docs.iter().map(|s| s.as_str()).collect();
    let serial: Vec<_> = refs.iter().map(|d| extract(d).map(|a| a.authors)).collect();
    let parallel: Vec<_> = extract_many(&refs)
        .into_iter()
        .map(|r| r.map(|a| a.authors))
        .collect();
    assert_eq!(serial.len(), parallel.len());
    for (s, p) in serial.iter().zip(parallel.iter()) {
        assert_eq!(s.is_ok(), p.is_ok());
        if let (Ok(s), Ok(p)) = (s, p) {
            assert_eq!(s.len(), p.len());
        }
    }
}

#[test]
fn serde_roundtrip() {
    let a = extract(BASIC_PAGE).unwrap();
    let json = serde_json::to_string(&a).unwrap();
    assert!(json.contains("\"title\":\"The Future of Rust Tooling\""));
}
