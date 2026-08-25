//! Async fetching (tokio + reqwest) — feature `http`.
//!
//! Port of newspaper3k's `network.py` behaviour: browser user agent,
//! timeouts, redirects, and content-type-aware text decoding (reqwest
//! handles gzip/deflate/brotli transparently via its features).

use crate::meta;
use crate::{extract_with_config_and_base, Article, Config, Error, Result};

const USER_AGENT: &str = concat!("nws/", env!("CARGO_PKG_VERSION"), " (+article-extraction)");

/// Browser-style user agents (subset of newspaper's `useragents.txt`).
pub const BROWSER_USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36",
    "Mozilla/5.0 (X11; Linux x86_64; rv:127.0) Gecko/20100101 Firefox/127.0",
];

/// Fetch options (newspaper `Configuration` fetch knobs).
#[derive(Debug, Clone)]
pub struct FetchOptions {
    /// Request timeout in seconds (newspaper `request_timeout`, default 7).
    pub timeout_secs: u64,
    /// Number of retries (newspaper image fetching retries; the page fetch
    /// itself is single-shot with a timeout).
    pub retries: u32,
    /// User agent; rotates through `BROWSER_USER_AGENTS` when unset.
    pub user_agent: Option<String>,
}

impl Default for FetchOptions {
    fn default() -> Self {
        FetchOptions {
            timeout_secs: 7,
            retries: 1,
            user_agent: None,
        }
    }
}

/// Port of `get_html_2XX_only`: fetch with timeout + redirects, error on
/// non-2xx, decode text with charset sniffing.
pub async fn fetch_html(url: &str) -> Result<String> {
    fetch_html_with(url, &FetchOptions::default()).await
}

/// Hard cap on response size (bytes) — content-length pre-check plus a
/// post-read check; a page beyond this is almost never an article.
pub const MAX_HTML_BYTES: usize = 10 * 1024 * 1024;

/// Shared client per (timeout, user-agent): one connection pool + TLS
/// session cache per configuration, reused across every fetch (pages and
/// image dimension checks).
pub(crate) fn shared_client(timeout_secs: u64, user_agent: &str) -> reqwest::Client {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CLIENTS: OnceLock<Mutex<HashMap<(u64, String), reqwest::Client>>> = OnceLock::new();
    let map = CLIENTS.get_or_init(|| Mutex::new(HashMap::new()));
    let key = (timeout_secs, user_agent.to_string());
    let mut guard = map.lock().expect("client cache");
    guard
        .entry(key)
        .or_insert_with(|| {
            reqwest::Client::builder()
                .user_agent(user_agent)
                .timeout(std::time::Duration::from_secs(timeout_secs))
                // Connection pooling + TLS session reuse across requests.
                .pool_max_idle_per_host(8)
                .build()
                .expect("http client")
        })
        .clone()
}

/// Fetch with explicit options. The HTTP client is shared per
/// (timeout, user-agent) — one TLS handshake per host, connections pooled.
pub async fn fetch_html_with(url: &str, opts: &FetchOptions) -> Result<String> {
    let ua = opts
        .user_agent
        .clone()
        .unwrap_or_else(|| USER_AGENT.to_string());
    let client = shared_client(opts.timeout_secs, &ua);
    let mut last_err: Option<Error> = None;

    for attempt in 0..=opts.retries {
        match client.get(url).send().await {
            Ok(resp) => {
                let status = resp.status();
                if !status.is_success() {
                    return Err(Error::Http(resp.error_for_status().unwrap_err()));
                }
                return read_html(resp).await;
            }
            Err(e) => {
                if attempt == opts.retries {
                    last_err = Some(Error::Http(e));
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| Error::Io(std::io::Error::other("fetch failed"))))
}

/// Read a response body with a hard byte cap enforced *while streaming*
/// (chunked responses and gzip bombs abort the moment they cross the cap —
/// never after buffering), then decode with the declared charset.
async fn read_html(resp: reqwest::Response) -> Result<String> {
    use futures_util::StreamExt;

    // Charset declared on the wire (Content-Type: …; charset=…).
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    if resp
        .content_length()
        .is_some_and(|len| len as usize > MAX_HTML_BYTES)
    {
        return Err(Error::Io(std::io::Error::other(format!(
            "page too large (>{MAX_HTML_BYTES} bytes)"
        ))));
    }

    let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(Error::Http)?;
        buf.extend_from_slice(&chunk);
        if buf.len() > MAX_HTML_BYTES {
            return Err(Error::Io(std::io::Error::other(format!(
                "page too large (>{MAX_HTML_BYTES} bytes)"
            ))));
        }
    }
    Ok(decode_html(buf, content_type.as_deref()))
}

/// Decode a fetched HTML body. The declared `Content-Type` charset wins;
/// otherwise the `<meta charset>`/`<meta http-equiv>` declaration in the
/// first 4 KiB is honoured; the final fallback is lossy UTF-8. This is the
/// "content-type-aware text decoding" newspaper3k's network layer does.
pub fn decode_html(bytes: Vec<u8>, content_type: Option<&str>) -> String {
    fn charset_from_content_type(ct: &str) -> Option<String> {
        let lower = ct.to_ascii_lowercase();
        let idx = lower.find("charset=")?;
        let rest = &ct[idx + "charset=".len()..];
        let label = rest
            .split(';')
            .next()
            .unwrap_or(rest)
            .trim()
            .trim_matches('"')
            .trim_matches('\'');
        if label.is_empty() {
            None
        } else {
            Some(label.to_string())
        }
    }

    if let Some(enc) = content_type
        .and_then(charset_from_content_type)
        .and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes()))
    {
        return enc.decode(&bytes).0.into_owned();
    }

    // Meta sniff: <meta charset="…"> or http-equiv content-type.
    let head_len = bytes.len().min(4096);
    let head = String::from_utf8_lossy(&bytes[..head_len]).to_ascii_lowercase();
    for cap in crate::regexes::meta_charset().captures_iter(&head) {
        if let Some(m) = cap.get(1) {
            if let Some(enc) = encoding_rs::Encoding::for_label(m.as_str().as_bytes()) {
                return enc.decode(&bytes).0.into_owned();
            }
        }
    }

    String::from_utf8_lossy(&bytes).into_owned()
}

/// Fetch and extract an article from a URL, resolving relative media/canonical
/// URLs against the article URL.
pub async fn extract_url(url: &str) -> Result<Article> {
    extract_url_with(url, &FetchOptions::default()).await
}

/// Fetch and extract with explicit fetch options. Extraction is CPU-bound,
/// so it runs on the blocking pool (`spawn_blocking`) — never on the async
/// executor's worker threads.
pub async fn extract_url_with(url: &str, opts: &FetchOptions) -> Result<Article> {
    let html = fetch_html_with(url, opts).await?;
    let url_owned = url.to_string();
    let mut article = tokio::task::spawn_blocking(move || {
        extract_with_config_and_base(&html, &Config::default(), Some(&url_owned))
    })
    .await
    .map_err(|e| Error::Io(std::io::Error::other(format!("worker join failed: {e}"))))??;
    // Resolve relative URLs in output fields.
    article.top_image = article
        .top_image
        .and_then(|img| meta::resolve(Some(url), &img));
    article.canonical_url = article
        .canonical_url
        .and_then(|c| meta::resolve(Some(url), &c));
    article.images = article
        .images
        .into_iter()
        .filter_map(|i| meta::resolve(Some(url), &i))
        .collect();
    if let Some(f) = article.favicon.as_ref() {
        article.favicon = meta::resolve(Some(url), f);
    }
    // Multi-page articles often link to relative next pages ("/story/2");
    // resolve so `multipage::extract_all_pages` can actually follow them.
    article.next_page = article.next_page.and_then(|n| meta::resolve(Some(url), &n));
    Ok(article)
}

#[cfg(test)]
mod decode_tests {
    use super::*;

    #[test]
    fn decodes_declared_shift_jis() {
        // "konnichiwa" encoded in Shift_JIS.
        let bytes: Vec<u8> = vec![0x82, 0xB1, 0x82, 0xF1, 0x82, 0xC9, 0x82, 0xBF, 0x82, 0xCD];
        let out = decode_html(bytes, Some("text/html; charset=shift_jis"));
        assert_eq!(out, "\u{3053}\u{3093}\u{306B}\u{3061}\u{306F}");
    }

    #[test]
    fn sniffs_meta_charset() {
        // "zhongwen" encoded in GBK, declared only in a <meta> tag.
        let head = "<html><head><meta charset=\"gbk\"></head><body>";
        let mut bytes = head.as_bytes().to_vec();
        bytes.extend_from_slice(&[0xD6, 0xD0, 0xCE, 0xC4]);
        let out = decode_html(bytes, None);
        assert!(out.contains("\u{4E2D}\u{6587}"), "got {out:?}");
    }

    #[test]
    fn falls_back_to_lossy_utf8() {
        let bytes: Vec<u8> = vec![b'c', b'a', b'f', 0xE9, 0xFF, 0x01];
        let out = decode_html(bytes, None);
        assert!(out.starts_with("caf"), "got {out:?}");
        assert!(out.contains('\u{FFFD}'), "invalid bytes must become U+FFFD");
    }

    #[test]
    fn quoted_charset_label_handled() {
        let bytes = "plain ascii".as_bytes().to_vec();
        let out = decode_html(bytes, Some("text/html; charset=\"utf-8\""));
        assert_eq!(out, "plain ascii");
    }
}
