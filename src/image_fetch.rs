//! Port of newspaper3k's `images.py` — the image-analysis pipeline:
//! fetch image dimensions over HTTP, score candidates by area/aspect/name,
//! and pick the largest qualifying image. Used to verify top-image
//! candidates when attribute-based scoring is inconclusive.

use crate::fetch::FetchOptions;
use futures_util::StreamExt;

/// newspaper `minimal_area`: ignore images smaller than this (px²).
const MINIMAL_AREA: u32 = 5000;
/// newspaper `thumbnail_size`: minimum width for a lead image.
const THUMBNAIL_MIN_WIDTH: u32 = 90;
/// First dimension-sniff checkpoint: image headers (SOF/IHDR/VP8) sit in
/// the first tens of KiB; if we can decode dimensions here we abort the
/// download and never pay for the pixels.
const FIRST_CHUNK_CAP: usize = 64 * 1024;
/// Second checkpoint for images with huge EXIF/XMP blocks before the SOF.
const RETRY_CAP: usize = 512 * 1024;
/// Hard cap on an image download (dimension sniffing only).
const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;
/// Bound on simultaneous candidate checks (per call).
const MAX_CONCURRENT_CHECKS: usize = 8;

fn parse_dimensions(buf: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(std::io::Cursor::new(buf))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

/// Fetch the dimensions of a remote image (partial read — we only need
/// the header, not the pixels). The HTTP client is the shared pooled one.
pub async fn fetch_image_dimension(url: &str, opts: &FetchOptions) -> Option<(u32, u32)> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return None;
    }
    let ua = opts.user_agent.clone().unwrap_or_default();
    let client =
        crate::fetch::shared_client(opts.timeout_secs, if ua.is_empty() { "nws" } else { &ua });
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let content_type = resp
        .headers()
        .get("content-type")?
        .to_str()
        .ok()?
        .to_string();
    if !content_type.contains("image") {
        return None;
    }
    if resp
        .content_length()
        .is_some_and(|len| len as usize > MAX_IMAGE_BYTES)
    {
        return None;
    }

    let mut buf: Vec<u8> = Vec::with_capacity(FIRST_CHUNK_CAP);
    let mut tried_first = false;
    let mut tried_retry = false;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.ok()?;
        buf.extend_from_slice(&chunk);
        if !tried_first && buf.len() >= FIRST_CHUNK_CAP {
            tried_first = true;
            if let Some(d) = parse_dimensions(&buf) {
                return Some(d);
            }
        }
        if !tried_retry && buf.len() >= RETRY_CAP {
            tried_retry = true;
            if let Some(d) = parse_dimensions(&buf) {
                return Some(d);
            }
        }
        if buf.len() >= MAX_IMAGE_BYTES {
            break;
        }
    }
    parse_dimensions(&buf)
}

/// Port of `Scraper.calculate_area`.
pub fn calculate_area(img_url: &str, dimension: Option<(u32, u32)>, max_ratio: f64) -> u32 {
    let Some((w, h)) = dimension else {
        return 0;
    };
    let mut area = w.saturating_mul(h);
    if area < MINIMAL_AREA {
        return 0;
    }
    if w < THUMBNAIL_MIN_WIDTH {
        return 0;
    }
    let current_ratio = w.max(h) as f64 / w.min(h).max(1) as f64;
    if current_ratio > max_ratio {
        return 0;
    }
    let lower = img_url.to_lowercase();
    if lower.contains("sprite") || lower.contains("logo") {
        area /= 10;
    }
    area
}

/// Port of `Scraper.largest_image_url`: check candidate images (up to
/// `MAX_CONCURRENT_CHECKS` at a time, on the shared client pool) and keep
/// the largest qualifying one.
pub async fn largest_image_url(
    candidates: &[String],
    _article_url: &str,
    max_ratio: f64,
    opts: &FetchOptions,
) -> Option<String> {
    let mut dims: Vec<Option<(u32, u32)>> = vec![None; candidates.len()];
    for (base, chunk) in candidates.chunks(MAX_CONCURRENT_CHECKS).enumerate() {
        let mut set = tokio::task::JoinSet::new();
        for (j, img) in chunk.iter().enumerate() {
            let url = img.clone();
            let opts = opts.clone();
            let idx = base * MAX_CONCURRENT_CHECKS + j;
            set.spawn(async move { (idx, fetch_image_dimension(&url, &opts).await) });
        }
        while let Some(res) = set.join_next().await {
            if let Ok((idx, d)) = res {
                dims[idx] = d;
            }
        }
    }

    let mut max_area: u32 = 0;
    let mut max_url: Option<String> = None;
    for (img, dim) in candidates.iter().zip(dims.iter()) {
        let area = calculate_area(img, *dim, max_ratio);
        if area > max_area {
            max_area = area;
            max_url = Some(img.clone());
        }
    }
    if max_url.is_some() {
        return max_url;
    }
    // Keep the first candidate when nothing is measurable (reference
    // behaviour leaves the pre-existing top image untouched).
    candidates.first().cloned()
}

/// Convenience: fetch + verify the top-image of an already-extracted
/// article against its candidate list.
pub async fn refine_top_image(article: &mut crate::Article, max_ratio: f64) {
    let opts = FetchOptions::default();
    if article.top_image.is_some() && article.images.is_empty() {
        return;
    }
    let mut candidates = article.images.clone();
    if let Some(t) = &article.top_image {
        candidates.insert(0, t.clone());
    }
    if candidates.is_empty() {
        return;
    }
    if let Some(best) = largest_image_url(&candidates, "", max_ratio, &opts).await {
        article.top_image = Some(best);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn area_rules() {
        assert_eq!(
            calculate_area("https://x.com/a.jpg", Some((800, 600)), 3.0),
            800 * 600
        );
        // Too small.
        assert_eq!(
            calculate_area("https://x.com/a.jpg", Some((40, 40)), 3.0),
            0
        );
        // Too skinny (below min width).
        assert_eq!(
            calculate_area("https://x.com/a.jpg", Some((80, 2000)), 3.0),
            0
        );
        // Bad aspect ratio.
        assert_eq!(
            calculate_area("https://x.com/a.jpg", Some((800, 10)), 3.0),
            0
        );
        // Sprite penalty.
        assert_eq!(
            calculate_area("https://x.com/sprite.png", Some((1000, 1000)), 3.0),
            1000 * 1000 / 10
        );
        // No dimension → zero.
        assert_eq!(calculate_area("https://x.com/a.jpg", None, 3.0), 0);
    }
}
