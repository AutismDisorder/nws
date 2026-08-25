//! Port of newspaper3k's `videos/extractors.py` — `VideoExtractor`: pull
//! video embeds (iframe/embed/object/video) out of the article top node and
//! tag them with their provider.

use crate::dom::{self, Handle};
use serde::Serialize;

/// The video tags scanned by the extractor.
const VIDEO_TAGS: &[&str] = &["IFRAME", "EMBED", "OBJECT", "VIDEO"];
/// Providers detected from the embed URL.
const VIDEO_PROVIDERS: &[&str] = &["youtube", "vimeo", "dailymotion", "kewego"];

/// Port of newspaper's `Video` object.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Video {
    /// Serialized outer HTML of the embed node.
    pub embed_code: String,
    /// Tag name of the embed node (`iframe`, `embed`, …).
    pub embed_type: String,
    pub width: Option<String>,
    pub height: Option<String>,
    pub src: Option<String>,
    /// Detected provider (`youtube`, `vimeo`, `dailymotion`, `kewego`).
    pub provider: Option<String>,
}

/// Port of `VideoExtractor.get_embed_code`: outer HTML, each line stripped.
fn get_embed_code(node: &Handle) -> String {
    dom::serialize(node)
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("")
}

fn get_embed_type(node: &Handle) -> String {
    dom::tag_name(node).unwrap_or_default().to_lowercase()
}

fn get_width(node: &Handle) -> Option<String> {
    dom::attr(node, "width")
}

fn get_height(node: &Handle) -> Option<String> {
    dom::attr(node, "height")
}

fn get_src(node: &Handle) -> Option<String> {
    dom::attr(node, "src")
}

fn get_provider(src: Option<&str>) -> Option<String> {
    let src = src?;
    VIDEO_PROVIDERS
        .iter()
        .find(|p| src.contains(**p))
        .map(|p| p.to_string())
}

/// Port of `VideoExtractor.get_video`.
fn get_video(node: &Handle) -> Video {
    let src = get_src(node);
    Video {
        embed_code: get_embed_code(node),
        embed_type: get_embed_type(node),
        width: get_width(node),
        height: get_height(node),
        provider: get_provider(src.as_deref()),
        src,
    }
}

/// Port of `get_object_tag`: read the `<param name="movie">` value, verify
/// the provider, and drop the child embed from the candidate list so it is
/// not double-parsed.
fn get_object_tag(node: &Handle, candidates: &mut Vec<Handle>) -> Option<Video> {
    // Remove a child embed from candidates (parsed once via the object).
    let child_embeds = dom::all_nodes_with_tag(node, &["EMBED"]);
    if let Some(embed) = child_embeds.first() {
        candidates.retain(|c| dom::id(c) != dom::id(embed));
    }

    // `<param name="movie" value="…">`
    let src_node = dom::all_nodes_with_tag(node, &["PARAM"])
        .into_iter()
        .find(|p| {
            dom::attr(p, "name")
                .as_deref()
                .is_some_and(|n| n.eq_ignore_ascii_case("movie"))
        })?;
    let src = dom::attr(&src_node, "value")?;
    let provider = get_provider(Some(&src))?;

    let mut video = get_video(node);
    video.provider = Some(provider);
    video.src = Some(src);
    Some(video)
}

/// Port of `VideoExtractor.get_embed_tag`: `<embed>` may sit inside an
/// `<object>`; in that case retrieve the object instead.
fn get_embed_tag(node: &Handle, candidates: &mut Vec<Handle>) -> Option<Video> {
    if let Some(parent) = dom::parent(node) {
        if dom::tag_is(&parent, "OBJECT") {
            return get_object_tag(&parent, candidates);
        }
    }
    Some(get_video(node))
}

/// Port of `VideoExtractor.get_videos`: scan the article top node for
/// embeds belonging to a known video provider. The candidate list is
/// iterated index-style and mutated live (like the Python `for` loop over
/// a list with `remove`), so `<embed>` children of `<object>` are not
/// double-parsed.
pub fn get_videos(top_node: &Handle) -> Vec<Video> {
    let mut candidates = dom::all_nodes_with_tag(top_node, VIDEO_TAGS);
    let mut movies = Vec::new();

    let mut i = 0;
    while i < candidates.len() {
        let candidate = candidates[i].clone();
        let tag = dom::tag_name(&candidate).unwrap_or_default();
        let movie = match tag.as_str() {
            "IFRAME" => Some(get_video(&candidate)),
            "VIDEO" => {
                // newspaper returns an empty Video for `<video>` tags
                // (provider unknown) — only keep with a detectable src.
                let v = get_video(&candidate);
                v.provider.is_some().then_some(v)
            }
            "EMBED" => get_embed_tag(&candidate, &mut candidates),
            "OBJECT" => get_object_tag(&candidate, &mut candidates),
            _ => None,
        };
        if let Some(movie) = movie {
            if movie.provider.is_some() {
                movies.push(movie);
            }
        }
        i += 1;
    }

    movies
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_youtube_iframe() {
        let dom = dom::parse(
            "<article><p>text</p><iframe width='560' height='315' \
             src='https://www.youtube.com/embed/dQw4w9WgXcQ'></iframe></article>",
        );
        let article = dom::all_nodes_with_tag(&dom::document(&dom), &["ARTICLE"])[0].clone();
        let videos = get_videos(&article);
        assert_eq!(videos.len(), 1);
        assert_eq!(videos[0].provider.as_deref(), Some("youtube"));
        assert_eq!(videos[0].embed_type, "iframe");
        assert_eq!(videos[0].width.as_deref(), Some("560"));
        assert!(videos[0].embed_code.contains("youtube.com/embed"));
    }

    #[test]
    fn extracts_object_with_movie_param() {
        let dom = dom::parse(
            "<article><object width='640' height='480'>\
             <param name='movie' value='https://www.youtube.com/v/dQw4w9WgXcQ'>\
             <embed src='https://www.youtube.com/v/dQw4w9WgXcQ'></embed></object></article>",
        );
        let article = dom::all_nodes_with_tag(&dom::document(&dom), &["ARTICLE"])[0].clone();
        let videos = get_videos(&article);
        assert_eq!(videos.len(), 1, "object+embed parsed once");
        assert_eq!(videos[0].provider.as_deref(), Some("youtube"));
        assert_eq!(
            videos[0].src.as_deref(),
            Some("https://www.youtube.com/v/dQw4w9WgXcQ")
        );
    }

    #[test]
    fn skips_unrelated_iframes() {
        let dom = dom::parse(
            "<article><iframe src='https://example.com/not-a-video'></iframe></article>",
        );
        let article = dom::all_nodes_with_tag(&dom::document(&dom), &["ARTICLE"])[0].clone();
        assert!(get_videos(&article).is_empty());
    }
}
