//! Port of newspaper3k's `urls.py` — URL purification (`prepare_url`,
//! `redirect_back`), file-type extraction, and the news-article URL
//! validator (`valid_url`) used to skip non-article pages cheaply.

use url::Url;

/// newspaper `ALLOWED_TYPES` — file types that can still be articles.
const ALLOWED_TYPES: &[&str] = &[
    "html", "htm", "md", "rst", "aspx", "jsp", "rhtml", "cgi", "xhtml", "jhtml", "asp", "shtml",
];

/// newspaper `GOOD_PATHS` — path keywords that signal articles.
const GOOD_PATHS: &[&str] = &[
    "story",
    "article",
    "feature",
    "featured",
    "slides",
    "slideshow",
    "gallery",
    "news",
    "video",
    "media",
    "v",
    "radio",
    "press",
];

/// newspaper `BAD_CHUNKS` — path/subdomain keywords that signal non-articles.
const BAD_CHUNKS: &[&str] = &[
    "careers",
    "contact",
    "about",
    "faq",
    "terms",
    "privacy",
    "advert",
    "preferences",
    "feedback",
    "info",
    "browse",
    "howto",
    "account",
    "subscribe",
    "donate",
    "shop",
    "admin",
];

/// newspaper `BAD_DOMAINS`.
const BAD_DOMAINS: &[&str] = &["amazon", "doubleclick", "twitter"];

/// Port of `get_domain` (netloc of the URL).
pub fn get_domain(url: &str) -> Option<String> {
    Url::parse(url)
        .ok()
        .map(|u| u.host_str().unwrap_or("").to_string())
}

/// Port of `url_to_filetype` ("https://x.com/a.jpg" → "jpg", none otherwise).
pub fn url_to_filetype(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let path = parsed.path().trim_end_matches('/');
    let last = path.rsplit('/').next()?;
    let parts: Vec<&str> = last.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let file_type = parts.last()?.to_lowercase();
    if file_type.len() <= 5 || ALLOWED_TYPES.contains(&file_type.as_str()) {
        Some(file_type)
    } else {
        None
    }
}

/// Port of `redirect_back`: URLs that redirect through another domain with
/// the real target in a `url` query param get unwrapped.
pub fn redirect_back(url: &str, source_domain: &str) -> String {
    let Ok(parsed) = Url::parse(url) else {
        return url.to_string();
    };
    let domain = parsed.host_str().unwrap_or("");
    if source_domain.contains(domain) || domain.contains(source_domain) {
        return url.to_string();
    }
    for (k, v) in parsed.query_pairs() {
        if k == "url" {
            return v.into_owned();
        }
    }
    url.to_string()
}

/// Port of `prepare_url`: resolve relative URLs against the source and
/// unwrap redirect shells.
pub fn prepare_url(url: &str, source_url: Option<&str>) -> String {
    let proper = match source_url {
        Some(source) => {
            let source_domain = Url::parse(source)
                .ok()
                .and_then(|u| u.host_str().map(str::to_string))
                .unwrap_or_default();
            let joined = Url::parse(source)
                .ok()
                .and_then(|s| s.join(url).ok())
                .map(|u| u.to_string())
                .unwrap_or_else(|| url.to_string());
            redirect_back(&joined, &source_domain)
        }
        None => url.to_string(),
    };
    proper
}

/// Port of `is_abs_url` (django-style absolute URL check).
pub fn is_abs_url(url: &str) -> bool {
    crate::regexes::abs_url().is_match(url)
}

/// Port of `valid_url`: cheap news-article URL validation.
pub fn valid_url(url: &str) -> bool {
    let url = prepare_url(url, None);

    // 11 chars is the shortest valid url length (http://x.co).
    if url.len() < 11 {
        return false;
    }
    if url.contains("mailto:") {
        return false;
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return false;
    }

    let Ok(parsed) = Url::parse(&url) else {
        return false;
    };
    let path = parsed.path();
    if !path.starts_with('/') {
        return false;
    }
    let path = path.trim_end_matches('/');

    let mut path_chunks: Vec<String> = path
        .split('/')
        .filter(|x| !x.is_empty())
        .map(str::to_string)
        .collect();

    // Siphon out the file type; reject media types instantly.
    if !path_chunks.is_empty() {
        if let Some(file_type) = url_to_filetype(&url) {
            if !ALLOWED_TYPES.contains(&file_type.as_str()) {
                return false;
            }
        }
        let last_chunk: Vec<&str> = path_chunks.last().unwrap().split('.').collect();
        if last_chunk.len() > 1 {
            *path_chunks.last_mut().unwrap() = last_chunk[last_chunk.len() - 2].to_string();
        }
    }

    // Index gives us no information.
    path_chunks.retain(|c| c != "index");

    let tld = tld_domain(&url);
    let subd = subdomain(&url);

    if BAD_DOMAINS.contains(&tld.as_str()) {
        return false;
    }

    let url_slug = path_chunks.last().cloned().unwrap_or_default();
    let dash_count = url_slug.matches('-').count();
    let underscore_count = url_slug.matches('_').count();

    // A news slug title: many separators and no domain mention.
    if !url_slug.is_empty() && (dash_count > 4 || underscore_count > 4) {
        if dash_count >= underscore_count && !url_slug.split('-').any(|x| x.to_lowercase() == tld) {
            return true;
        }
        if underscore_count > dash_count && !url_slug.split('_').any(|x| x.to_lowercase() == tld) {
            return true;
        }
    }

    // There must be at least 2 subpaths.
    if path_chunks.len() <= 1 {
        return false;
    }

    for b in BAD_CHUNKS {
        if path_chunks.iter().any(|c| c == b) || subd == *b {
            return false;
        }
    }

    // A date pattern in the URL is a very safe bet.
    if crate::regexes::url_date().is_match(&url) {
        return true;
    }

    for good in GOOD_PATHS {
        if path_chunks.iter().any(|p| p.to_lowercase() == *good) {
            return true;
        }
    }

    false
}

/// Registrable domain *label* — tldextract's `.domain` ("example" from
/// "https://news.example.com/x"). newspaper compares BAD_DOMAINS and URL-slug
/// chunks against this label, NOT the full two-label suffix ("example.com"
/// can never equal "twitter" — the old form made both checks dead code).
fn tld_domain(url: &str) -> String {
    let Ok(parsed) = Url::parse(url) else {
        return String::new();
    };
    let host = parsed.host_str().unwrap_or("").to_string();
    // Approximate tldextract without a PSL: second-to-last label.
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() >= 2 {
        labels[labels.len() - 2].to_string()
    } else {
        host
    }
}

fn subdomain(url: &str) -> String {
    let Ok(parsed) = Url::parse(url) else {
        return String::new();
    };
    let host = parsed.host_str().unwrap_or("").to_string();
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() >= 3 {
        labels[..labels.len() - 2].join(".")
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetype_extraction() {
        assert_eq!(
            url_to_filetype("https://x.com/images/car.jpg").as_deref(),
            Some("jpg")
        );
        assert_eq!(url_to_filetype("https://x.com/story/page"), None);
        assert_eq!(
            url_to_filetype("https://x.com/story/page.html").as_deref(),
            Some("html")
        );
    }

    #[test]
    fn redirect_back_unwraps() {
        // Only fires when the domains are unrelated (the reference skips
        // anything on the same domain or subdomain).
        assert_eq!(
            redirect_back(
                "https://redirector.example.com?url=https%3A%2F%2Freal.com%2Fstory",
                "google.com"
            ),
            "https://real.com/story"
        );
        assert_eq!(
            redirect_back("https://example.com/x", "example.com"),
            "https://example.com/x"
        );
    }

    #[test]
    fn valid_url_accepts_articles() {
        assert!(valid_url(
            "https://cnn.com/2026/08/15/some-story-name/index.html"
        ));
        assert!(valid_url(
            "https://example.com/story/my-great-article-with-many-words"
        ));
        assert!(valid_url("https://example.com/news/politics"));
    }

    #[test]
    fn valid_url_rejects_junk() {
        assert!(!valid_url("https://example.com/careers"));
        assert!(!valid_url("https://example.com/about.html"));
        assert!(!valid_url("https://example.com"));
        assert!(!valid_url("https://example.com/image.png"));
        assert!(!valid_url("mailto:x@example.com"));
    }

    #[test]
    fn abs_url_check() {
        assert!(is_abs_url("https://example.com/a"));
        assert!(is_abs_url("http://localhost:3000"));
        assert!(!is_abs_url("/relative/path"));
    }
}
