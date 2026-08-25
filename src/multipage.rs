//! Port of mercury's `collect-all-pages.js` — follow `next_page_url`
//! links (up to 25 follow-ups), join the pages with `<hr><h4>Page N</h4>`
//! markers, and recount the words — the multi-page article path.

#[cfg(feature = "http")]
use crate::fetch::{extract_url_with, FetchOptions};
#[cfg(feature = "http")]
use crate::pagination;
use crate::Article;
#[cfg(feature = "http")]
use crate::Result;

/// Info about a multi-page assembly.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct MultiPage {
    /// Pages actually fetched and merged (1 = single page, no follow-ups).
    pub total_pages: usize,
    pub rendered_pages: usize,
}

fn merge_page(base: &mut Article, next: Article, page: usize) {
    let sep = format!("<hr><h4>Page {}</h4>", page + 1);
    base.html = format!("{}{}{}", base.html, sep, next.html);
    base.text = format!("{}\n\nPage {}\n\n{}", base.text, page + 1, next.text);
    base.markdown = format!(
        "{}\n\n---\n\n## Page {}\n\n{}",
        base.markdown,
        page + 1,
        next.markdown
    );
    // Keep page-1 metadata; extend media lists.
    for img in next.images {
        if !base.images.contains(&img) {
            base.images.push(img);
        }
    }
    for v in next.videos {
        if !base.videos.contains(&v) {
            base.videos.push(v);
        }
    }
}

/// Fetch an article and follow its next-page links, merging content —
/// the port of `collectAllPages`. Returns the merged article plus page
/// stats. Single-page articles return with `total_pages == 1`.
#[cfg(feature = "http")]
pub async fn extract_all_pages(url: &str, opts: &FetchOptions) -> Result<(Article, MultiPage)> {
    let mut article = extract_url_with(url, opts).await?;
    let mut info = MultiPage {
        total_pages: 1,
        rendered_pages: 1,
    };

    let mut previous_urls: Vec<String> = vec![pagination::remove_anchor(url)];
    let mut next_url = article.next_page.clone();

    while let Some(next) = next_url.clone() {
        if info.total_pages >= 26 {
            break;
        }
        info.total_pages += 1;

        let mut next_article = match extract_url_with(&next, opts).await {
            Ok(a) => a,
            Err(_) => break,
        };
        previous_urls.push(pagination::remove_anchor(&next));
        next_url = next_article.next_page.take();
        // Skip links we have already fetched (loop guard).
        if next_url.as_deref().is_some_and(|u| {
            previous_urls
                .iter()
                .any(|p| p == &pagination::remove_anchor(u))
        }) {
            next_url = None;
        }

        merge_page(&mut article, next_article, info.total_pages - 1);
        info.rendered_pages = info.total_pages;
    }

    article.word_count = article.text.split_whitespace().count();
    article.next_page = next_url;
    Ok((article, info))
}

/// Port of `collectAllPages` (offline part): merge already-extracted pages.
pub fn merge_pages(base: &mut Article, pages: Vec<Article>) {
    for (i, page) in pages.into_iter().enumerate() {
        merge_page(base, page, i + 1);
    }
    base.word_count = base.text.split_whitespace().count();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(title: &str, body: &str) -> Article {
        Article {
            title: title.to_string(),
            text: body.to_string(),
            markdown: body.to_string(),
            html: format!("<div><p>{}</p></div>", body),
            ..Default::default()
        }
    }

    #[test]
    fn merge_pages_joins_with_markers() {
        let mut base = sample("t", "first page content");
        let pages = vec![sample("t", "second page content")];
        merge_pages(&mut base, pages);
        assert!(base.text.contains("Page 2"));
        assert!(base.text.contains("second page content"));
        assert!(base.markdown.contains("## Page 2"));
        assert!(base.html.contains("<hr><h4>Page 2</h4>"));
    }
}
