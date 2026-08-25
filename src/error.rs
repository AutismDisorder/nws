//! Error types for the `nws` article-extraction engine.

use thiserror::Error;

/// Errors that can occur while extracting an article.
#[derive(Debug, Error)]
pub enum Error {
    /// The HTML document failed to parse.
    #[error("html parse failed: {0}")]
    Parse(String),

    /// The document parsed, but no article could be found in it.
    #[error("no article content could be extracted from this document")]
    NotExtractable,

    /// A URL was malformed.
    #[error("invalid url: {0}")]
    Url(#[from] url::ParseError),

    /// I/O error (e.g. while fetching a page).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Network error while fetching a page (requires the `http` feature).
    #[cfg(feature = "http")]
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
}

/// Convenience result alias.
pub type Result<T> = std::result::Result<T, Error>;
