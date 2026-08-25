//! `nws` CLI — extract an article from a URL or a local HTML file.
//!
//! ```text
//! nws https://example.com/article        # full JSON article
//! nws --markdown https://example.com/article   # just the Markdown body
//! nws ./page.html
//! ```

use std::env;

fn main() {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    let markdown_only = args.first().is_some_and(|a| a == "--markdown" || a == "-m");
    if markdown_only {
        args.remove(0);
    }

    let Some(arg) = args.first() else {
        eprintln!("usage: nws [--markdown] <url|file.html>");
        std::process::exit(2);
    };

    let start = std::time::Instant::now();
    let result = if arg.starts_with("http://") || arg.starts_with("https://") {
        run_async_url(arg)
    } else {
        let html = std::fs::read_to_string(arg).unwrap_or_else(|e| {
            eprintln!("read error: {e}");
            std::process::exit(1);
        });
        nws::extract(&html)
    };

    match result {
        Ok(article) => {
            use std::io::Write;
            let out = if markdown_only {
                article.markdown.clone()
            } else {
                serde_json::to_string_pretty(&article).expect("article serializes")
            };
            // Write defensively: `nws url | head` must not panic on EPIPE.
            let stdout = std::io::stdout();
            let mut lock = stdout.lock();
            if lock.write_all(out.as_bytes()).is_err() || lock.write_all(b"\n").is_err() {
                return;
            }
            eprintln!("extracted in {:?}", start.elapsed());
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(feature = "http")]
fn run_async_url(url: &str) -> nws::Result<nws::Article> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(nws::fetch::extract_url(url))
}

#[cfg(not(feature = "http"))]
fn run_async_url(_url: &str) -> nws::Result<nws::Article> {
    Err(nws::Error::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "http feature disabled",
    )))
}
