//! `nws-server` — the article-extraction API.
//!
//! The self-hostable HTTP endpoint behind the engine: feed it a URL or raw
//! HTML, get back clean JSON (or Markdown / article HTML via `Accept`).
//! Batch extraction runs in parallel: URL fetches concurrently (tokio,
//! bounded), extraction across all cores (rayon).
//!
//! Build: `cargo build --release --features server`
//!
//! ```text
//! GET  /health                     -> {"status":"ok","version":…}
//! GET  /extract?url=<url>          -> JSON article
//! GET  /extract?url=<url>&multipage=1  -> follow next-page links, merge
//! GET  /readerable?url=<url>       -> cheap is-it-an-article check
//! POST /extract                    -> body: {"url": …} or {"html": …} or raw HTML
//! POST /batch                      -> body: {"urls":[…] } or {"docs":[{"html":…}, …]}
//! ```
//!
//! Content negotiation on `/extract`: `Accept: text/markdown` returns the
//! article as Markdown; `Accept: text/html` returns the cleaned HTML;
//! anything else returns JSON.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

/// Bound on simultaneous upstream fetches inside one `/batch` request.
const BATCH_FETCH_CONCURRENCY: usize = 16;
/// Cap on the number of items (urls + docs) per `/batch` request.
const BATCH_MAX_ITEMS: usize = 256;
/// Cap on aggregate fetched HTML per `/batch` request (64 MiB).
const BATCH_MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
/// Default upstream fetch timeout (overridable via NWS_FETCH_TIMEOUT).
const FETCH_TIMEOUT_SECS: u64 = 7;

// ---------------------------------------------------------------- state

#[derive(Clone)]
struct AppState {
    fetch_client: Arc<reqwest::Client>,
}

// ---------------------------------------------------------------- payloads

#[derive(Debug, Deserialize, Default)]
struct ExtractQuery {
    url: String,
    /// Follow next-page links and merge (mercury collectAllPages).
    /// Any non-empty value enables it: `multipage=1` or `multipage=true`.
    multipage: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ExtractBody {
    url: Option<String>,
    html: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct BatchBody {
    urls: Option<Vec<String>>,
    docs: Option<Vec<DocBody>>,
}

#[derive(Debug, Deserialize)]
struct DocBody {
    html: String,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Serialize)]
struct BatchResult {
    total: usize,
    ok: usize,
    results: Vec<serde_json::Value>,
}

// ---------------------------------------------------------------- helpers

fn error_response(status: StatusCode, msg: &str) -> Response {
    (status, Json(serde_json::json!({ "error": msg }))).into_response()
}

async fn fetch_html(client: &reqwest::Client, url: &str) -> Result<String, String> {
    use futures_util::StreamExt;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("fetch failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("upstream status {}", resp.status()));
    }
    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let max_bytes = nws::fetch::MAX_HTML_BYTES;
    if resp
        .content_length()
        .is_some_and(|len| len as usize > max_bytes)
    {
        return Err(format!("page too large (>{max_bytes} bytes)"));
    }
    // Stream with the cap enforced per chunk — chunked responses and gzip
    // bombs abort the moment they cross MAX_HTML_BYTES, never after
    // buffering the whole decompressed body.
    let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("read failed: {e}"))?;
        buf.extend_from_slice(&chunk);
        if buf.len() > max_bytes {
            return Err(format!("page too large (>{max_bytes} bytes)"));
        }
    }
    Ok(nws::fetch::decode_html(buf, content_type.as_deref()))
}

/// 502 for upstream/network problems, 422 for extraction failures.
fn status_for_error(e: &str) -> StatusCode {
    if e.starts_with("extraction failed") || e.contains("worker join failed") {
        StatusCode::UNPROCESSABLE_ENTITY
    } else {
        StatusCode::BAD_GATEWAY
    }
}

fn extract_html(html: &str, base: Option<&str>) -> Result<nws::Article, String> {
    nws::extract_with_config_and_base(html, &nws::Config::default(), base)
        .map_err(|e| format!("extraction failed: {e}"))
}

/// Honour `Accept`: markdown / cleaned html / JSON.
fn article_response(article: &nws::Article, headers: &HeaderMap) -> Response {
    let accept = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if accept.contains("text/markdown") {
        (
            [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
            article.markdown.clone(),
        )
            .into_response()
    } else if accept.contains("text/html") {
        (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            article.html.clone(),
        )
            .into_response()
    } else {
        Json(article).into_response()
    }
}

// ---------------------------------------------------------------- handlers

async fn health(State(state): State<AppState>) -> Response {
    let _ = &state;
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "engine": "newspaper3k/mercury/readability lineage",
    }))
    .into_response()
}

async fn extract_query(
    State(state): State<AppState>,
    Query(q): Query<ExtractQuery>,
    headers: HeaderMap,
) -> Response {
    if q.multipage.as_deref().is_some_and(|v| !v.is_empty()) {
        match nws::multipage::extract_all_pages(&q.url, &nws::fetch::FetchOptions::default()).await
        {
            Ok((article, info)) => {
                let mut value = serde_json::to_value(&article).unwrap_or_default();
                value["total_pages"] = serde_json::json!(info.total_pages);
                value["rendered_pages"] = serde_json::json!(info.rendered_pages);
                Json(value).into_response()
            }
            Err(e) => error_response(status_for_error(&format!("{e}")), &format!("{e}")),
        }
    } else {
        match extract_from_url(&state, &q.url).await {
            Ok(article) => article_response(&article, &headers),
            Err(e) => error_response(status_for_error(&e), &e),
        }
    }
}

async fn readerable_query(
    State(_state): State<AppState>,
    Query(q): Query<ExtractQuery>,
) -> Response {
    match nws::fetch::fetch_html(&q.url).await {
        Ok(html) => {
            let readerable =
                tokio::task::spawn_blocking(move || nws::readerable::is_readerable_html(&html))
                    .await
                    .unwrap_or(false);
            Json(serde_json::json!({
                "url": q.url,
                "readerable": readerable,
            }))
            .into_response()
        }
        Err(e) => error_response(StatusCode::BAD_GATEWAY, &format!("{e}")),
    }
}

async fn extract_body(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<ExtractBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(b) => b,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "invalid JSON body");
        }
    };

    let url = body.url.clone();
    let html = body.html.clone();

    let article = if let Some(u) = url.as_deref() {
        match extract_from_url(&state, u).await {
            Ok(a) => a,
            Err(e) => return error_response(status_for_error(&e), &e),
        }
    } else if let Some(html) = html {
        let base = url.clone();
        let result =
            tokio::task::spawn_blocking(move || extract_html(&html, base.as_deref())).await;
        match result {
            Ok(Ok(a)) => a,
            Ok(Err(e)) => {
                return error_response(StatusCode::UNPROCESSABLE_ENTITY, &e);
            }
            Err(join_err) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("worker join failed: {join_err}"),
                );
            }
        }
    } else {
        return error_response(StatusCode::BAD_REQUEST, "body must contain `url` or `html`");
    };

    article_response(&article, &headers)
}

async fn extract_from_url(state: &AppState, url: &str) -> Result<nws::Article, String> {
    let html = fetch_html(&state.fetch_client, url).await?;
    let html_owned = html;
    let url_owned = url.to_string();
    let result = tokio::task::spawn_blocking(move || extract_html(&html_owned, Some(&url_owned)))
        .await
        .map_err(|e| format!("worker join failed: {e}"))?;
    result
}

async fn batch(State(state): State<AppState>, Json(body): Json<BatchBody>) -> Response {
    let mut docs: Vec<(String, Option<String>)> = Vec::new();

    let item_count = body.urls.as_ref().map(Vec::len).unwrap_or(0)
        + body.docs.as_ref().map(Vec::len).unwrap_or(0);
    if item_count > BATCH_MAX_ITEMS {
        return error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            &format!("batch too large: {item_count} items (max {BATCH_MAX_ITEMS})"),
        );
    }

    if let Some(urls) = &body.urls {
        // Fetch all URLs concurrently (tokio), bounded by a semaphore, on
        // the shared connection pool; results re-ordered to input order.
        let sem = Arc::new(Semaphore::new(BATCH_FETCH_CONCURRENCY));
        let mut set = tokio::task::JoinSet::new();
        for (i, url) in urls.iter().enumerate() {
            let client = Arc::clone(&state.fetch_client);
            let url = url.clone();
            let sem = Arc::clone(&sem);
            set.spawn(async move {
                let _permit = sem.acquire().await.expect("semaphore");
                let fetched = fetch_html(&client, &url).await;
                (i, fetched)
            });
        }
        let mut by_index: Vec<Option<(String, Option<String>)>> = vec![None; urls.len()];
        let mut total_bytes = 0usize;
        while let Some(res) = set.join_next().await {
            let Ok((i, fetched)) = res else { continue };
            by_index[i] = Some(match fetched {
                Ok(html) => {
                    total_bytes += html.len();
                    if total_bytes > BATCH_MAX_TOTAL_BYTES {
                        return error_response(
                            StatusCode::PAYLOAD_TOO_LARGE,
                            &format!("batch exceeds {BATCH_MAX_TOTAL_BYTES} bytes of fetched HTML"),
                        );
                    }
                    (html, Some(urls[i].clone()))
                }
                Err(e) => (format!("__error__: {e}"), Some(urls[i].clone())),
            });
        }
        docs.extend(by_index.into_iter().flatten());
    }
    if let Some(d) = &body.docs {
        for doc in d {
            docs.push((doc.html.clone(), doc.url.clone()));
        }
    }
    if docs.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "body must contain `urls` or `docs`",
        );
    }

    let total = docs.len();
    // rayon: extract the whole batch in parallel across all cores (inside
    // the blocking pool so the async executor stays free for I/O).
    let results = tokio::task::spawn_blocking(move || {
        docs.par_iter()
            .map(|(html, base)| {
                if html.starts_with("__error__: ") {
                    return serde_json::json!({
                        "error": html.trim_start_matches("__error__: ")
                    });
                }
                match extract_html(html, base.as_deref()) {
                    Ok(article) => serde_json::to_value(&article)
                        .unwrap_or(serde_json::json!({"error": "serialize failed"})),
                    Err(e) => serde_json::json!({ "error": e }),
                }
            })
            .collect::<Vec<_>>()
    })
    .await;

    let results = match results {
        Ok(r) => r,
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &format!("{e}")),
    };

    let ok = results.iter().filter(|r| r.get("error").is_none()).count();
    Json(BatchResult { total, ok, results }).into_response()
}

// ---------------------------------------------------------------- main

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("NWS_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    let timeout_secs: u64 = std::env::var("NWS_FETCH_TIMEOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(FETCH_TIMEOUT_SECS);
    // Total request deadline: a slow upstream must never hang a handler (or
    // leak a `/batch` semaphore permit) forever.
    let fetch_client = reqwest::Client::builder()
        .user_agent(concat!("nws-server/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .expect("http client");

    let state = AppState {
        fetch_client: Arc::new(fetch_client),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/extract", get(extract_query).post(extract_body))
        .route("/readerable", get(readerable_query))
        .route("/batch", post(batch))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind address");
    println!(
        "nws-server {} listening on http://{addr}",
        env!("CARGO_PKG_VERSION")
    );
    axum::serve(listener, app).await.expect("server");
}
