//! Output formatting: newspaper3k's `outputformatters.py` port (text/HTML
//! cleanup) plus a Markdown renderer over the cleaned article tree — the
//! LLM-ingestion output the whole engine exists for.

use crate::dom::{self, Handle};
use url::Url;

// ---------------------------------------------------------------- text

/// Port of `OutputFormatter.get_formatted` text half:
/// strip link tags, br/li newlines, replace formatting tags with text,
/// drop empty tags and the trailing media div, then join paragraphs.
pub fn format_text(content: &Handle) -> String {
    remove_negative_score_nodes(content);
    links_to_text(content);
    add_newline_to_br(content);
    add_newline_to_li(content);
    replace_with_text(content);
    remove_empty_tags(content);
    remove_trailing_media_div(content);
    convert_to_text(content)
}

/// Port of `remove_negativescores_nodes`.
fn remove_negative_score_nodes(top: &Handle) {
    let mut bad = Vec::new();
    for node in dom::all_elements(top) {
        if let Some(score) = dom::attr(&node, "gravityscore") {
            let score: f64 = score.parse().unwrap_or(0.0);
            if score < 1.0 {
                bad.push(node);
            }
        }
    }
    for node in bad {
        dom::detach(&node);
    }
}

/// Port of `links_to_text`: unwrap every `<a>` (keep the text).
fn links_to_text(top: &Handle) {
    for a in dom::all_nodes_with_tag(top, &["A"]) {
        dom::unwrap(&a);
    }
}

/// Port of `add_newline_to_br`: `<br>` becomes a `\n` text node.
fn add_newline_to_br(top: &Handle) {
    for br in dom::all_nodes_with_tag(top, &["BR"]) {
        let text = dom::create_text("\n");
        dom::replace_node(&br, &text);
    }
}

/// Port of `add_newline_to_li`: each `<li>` (except the last in its list)
/// gets a trailing `\n`; nested content is collapsed into text.
fn add_newline_to_li(top: &Handle) {
    for ul in dom::all_nodes_with_tag(top, &["UL"]) {
        let lis = dom::children(&ul);
        let n = lis.len();
        for (i, li) in lis.into_iter().enumerate() {
            if i == n - 1 {
                continue;
            }
            let text = dom::create_text(&format!("{}\n", dom::inner_text(&li, true)));
            let kids = dom::child_nodes(&li);
            for c in kids {
                dom::detach(&c);
            }
            dom::append_child(&li, &text);
        }
    }
}

/// Port of `replace_with_text`: unwrap b/strong/i/br/sup (keep contents).
fn replace_with_text(top: &Handle) {
    for tag in ["B", "STRONG", "I", "BR", "SUP"] {
        for node in dom::all_nodes_with_tag(top, &[tag]) {
            dom::unwrap(&node);
        }
    }
}

/// Port of `remove_empty_tags`: drop elements with no text (keeping
/// object/embed containers).
fn remove_empty_tags(top: &Handle) {
    let mut nodes = dom::all_elements(top);
    nodes.reverse();
    for el in nodes {
        let tag = dom::tag_name(&el).unwrap_or_default();
        let text = dom::inner_text(&el, false);
        let has_object = !dom::all_nodes_with_tag(&el, &["OBJECT"]).is_empty();
        let has_embed = !dom::all_nodes_with_tag(&el, &["EMBED"]).is_empty();
        if text.trim().is_empty() && !has_object && !has_embed {
            if tag == "BR" && text.contains('\r') {
                continue;
            }
            dom::detach(&el);
        }
    }
}

/// Port of `remove_trailing_media_div`: if the last top-level child of the
/// article container is a deep DOM nest (a media/related-content cluster),
/// drop it.
fn remove_trailing_media_div(top: &Handle) {
    const NON_MEDIA_CLASSES: &[&str] = &["zn-body__read-all"];

    fn get_depth(node: &Handle, depth: usize) -> usize {
        let children = dom::children(node);
        if children.is_empty() {
            depth
        } else {
            children
                .iter()
                .map(|c| get_depth(c, depth + 1))
                .max()
                .unwrap_or(depth)
        }
    }

    let top_level = dom::children(top);
    if top_level.len() < 3 {
        return;
    }
    let last = top_level[top_level.len() - 1].clone();
    if NON_MEDIA_CLASSES.contains(&dom::class_name(&last).as_str()) {
        return;
    }
    if get_depth(&last, 1) >= 2 {
        dom::detach(&last);
    }
}

/// Port of `convert_to_text`: per-top-level-child text, trimmed and joined
/// with blank lines. Iterates *all* children (text nodes included), like
/// `list(node)` in lxml.
fn convert_to_text(top: &Handle) -> String {
    let mut txts: Vec<String> = Vec::new();
    for node in dom::child_nodes(top) {
        let txt = dom::inner_text(&node, true);
        if txt.is_empty() {
            continue;
        }
        let lines: Vec<&str> = txt
            .split('\n')
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        txts.extend(lines.iter().map(|s| s.to_string()));
    }
    txts.join("\n\n")
}

// ---------------------------------------------------------------- markdown

/// Render the cleaned article tree as Markdown.
///
/// Relative links and images are resolved against `base_url` when given
/// (the resolved URL of the fetched page; see `make_links_absolute`).
pub fn to_markdown(content: &Handle, base_url: Option<&str>) -> String {
    let mut out = String::new();
    render_node(content, base_url, 0, &mut out);
    collapse_blank_lines(&out)
}

fn render_children(node: &Handle, base: Option<&str>, depth: usize, out: &mut String) {
    for child in dom::children(node) {
        render_node(&child, base, depth, out);
    }
}

fn render_node(node: &Handle, base: Option<&str>, depth: usize, out: &mut String) {
    if dom::is_text(node) {
        out.push_str(normalize_inline(&dom::text_content(node)).as_str());
        return;
    }
    let Some(tag) = dom::tag_name(node) else {
        return;
    };
    let tag = tag.as_str();

    match tag {
        "SCRIPT" | "STYLE" | "NOSCRIPT" | "TEMPLATE" | "LINK" | "META" => {}
        "H1" | "H2" | "H3" | "H4" | "H5" | "H6" => {
            let level = tag.as_bytes()[1] as usize - b'0' as usize;
            let text = inline_markdown(node, base);
            if !text.trim().is_empty() {
                out.push_str(&format!("\n\n{} {}\n\n", "#".repeat(level), text));
            }
        }
        "P" => {
            let text = inline_markdown(node, base);
            if !text.trim().is_empty() {
                out.push_str(&format!("\n\n{}\n\n", text.trim()));
            }
        }
        "PRE" => {
            let code = dom::inner_text(node, false);
            out.push_str(&format!("\n\n```\n{}\n```\n\n", code.trim()));
        }
        "BLOCKQUOTE" => {
            let mut inner = String::new();
            render_children(node, base, depth + 1, &mut inner);
            for line in inner.split('\n') {
                if line.trim().is_empty() {
                    out.push_str(">\n");
                } else {
                    out.push_str(&format!("> {}\n", line));
                }
            }
            out.push_str("\n\n");
        }
        "UL" | "OL" => render_list(node, base, depth, tag == "OL", out),
        "TABLE" => render_table(node, base, out),
        "IMG" => {
            let src = resolve(dom::attr(node, "src").as_deref(), base);
            let alt = dom::attr(node, "alt").unwrap_or_default();
            if !src.is_empty() {
                out.push_str(&format!("\n\n![{}]({})\n\n", escape_brackets(&alt), src));
            }
        }
        "FIGURE" | "PICTURE" => {
            // Image first, then figcaption as emphasis.
            for img in dom::all_nodes_with_tag(node, &["IMG"]) {
                render_node(&img, base, depth, out);
            }
            for cap in dom::all_nodes_with_tag(node, &["FIGCAPTION"]) {
                let text = inline_markdown(&cap, base);
                if !text.trim().is_empty() {
                    out.push_str(&format!("\n\n*{}*\n\n", text.trim()));
                }
            }
        }
        "HR" => out.push_str("\n\n---\n\n"),
        "BR" => out.push('\n'),
        "A" => {
            let text = inline_markdown(node, base).trim().to_string();
            let href = resolve(dom::attr(node, "href").as_deref(), base);
            if href.is_empty() {
                out.push_str(&text);
            } else {
                out.push_str(&format!("[{}]({})", text, href));
            }
        }
        "STRONG" | "B" => out.push_str(&format!("**{}**", inline_markdown(node, base).trim())),
        "EM" | "I" => out.push_str(&format!("*{}*", inline_markdown(node, base).trim())),
        "DEL" | "S" | "STRIKE" => {
            out.push_str(&format!("~~{}~~", inline_markdown(node, base).trim()))
        }
        "CODE" => out.push_str(&format!("`{}`", dom::inner_text(node, false).trim())),
        "IFRAME" | "VIDEO" | "OBJECT" | "EMBED" => {
            if let Some(src) = dom::attr(node, "src") {
                if !src.is_empty() {
                    out.push_str(&format!("\n\n<iframe src=\"{}\"></iframe>\n\n", src));
                }
            }
        }
        _ => render_children(node, base, depth, out),
    }
}

fn render_list(node: &Handle, base: Option<&str>, depth: usize, ordered: bool, out: &mut String) {
    out.push_str("\n\n");
    let items = dom::children(node);
    for (i, li) in items.iter().enumerate() {
        let marker = if ordered {
            format!("{}. ", i + 1)
        } else {
            "- ".to_string()
        };
        let indent = "  ".repeat(depth);
        // Inline text of the li EXCLUDING nested lists — those render as
        // their own indented blocks below; including them here duplicated
        // their text.
        let mut text = String::new();
        for child in dom::child_nodes(li) {
            if dom::tag_name(&child)
                .as_deref()
                .is_some_and(|t| t == "UL" || t == "OL")
            {
                continue;
            }
            if dom::is_text(&child) {
                text.push_str(&normalize_inline(&dom::text_content(&child)));
            } else {
                text.push_str(&inline_markdown(&child, base));
            }
        }
        out.push_str(&format!("{}{}{}\n", indent, marker, text.trim()));

        // Nested lists: render with deeper indent (already indented by the
        // recursive call — no extra prefix).
        let mut nested = String::new();
        for child in dom::children(li) {
            if let Some(t) = dom::tag_name(&child) {
                if t == "UL" || t == "OL" {
                    render_list(&child, base, depth + 1, t == "OL", &mut nested);
                }
            }
        }
        for line in nested.lines() {
            if !line.trim().is_empty() {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out.push('\n');
}

fn render_table(node: &Handle, base: Option<&str>, out: &mut String) {
    let rows = dom::all_nodes_with_tag(node, &["TR"]);
    if rows.is_empty() {
        return;
    }
    let mut md_rows: Vec<Vec<String>> = Vec::new();
    for row in &rows {
        let mut cells = Vec::new();
        for cell in dom::child_nodes(row) {
            if let Some(t) = dom::tag_name(&cell) {
                if t == "TH" || t == "TD" {
                    cells.push(inline_markdown(&cell, base).trim().to_string());
                }
            }
        }
        if !cells.is_empty() {
            md_rows.push(cells);
        }
    }
    if md_rows.is_empty() {
        return;
    }
    let ncols = md_rows.iter().map(Vec::len).max().unwrap_or(1);

    let mut has_header = !dom::all_nodes_with_tag(node, &["TH"]).is_empty();
    let mut lines: Vec<String> = Vec::new();
    for (i, row) in md_rows.iter().enumerate() {
        let mut padded = row.clone();
        padded.resize(ncols, String::new());
        lines.push(format!("| {} |", padded.join(" | ")));
        if has_header && i == 0 {
            lines.push(format!("| {} |", vec!["---"; ncols].join(" | ")));
            has_header = false;
        }
    }
    if has_header {
        // No TH row found: use the first row as header.
        lines.insert(1, format!("| {} |", vec!["---"; ncols].join(" | ")));
    }
    out.push_str(&format!("\n\n{}\n\n", lines.join("\n")));
}

/// Render inline content (text + phrasing elements) of `node` as Markdown.
fn inline_markdown(node: &Handle, base: Option<&str>) -> String {
    let mut out = String::new();
    for child in dom::child_nodes(node) {
        if dom::is_text(&child) {
            out.push_str(&normalize_inline(&dom::text_content(&child)));
        } else if let Some(tag) = dom::tag_name(&child) {
            match tag.as_str() {
                "SCRIPT" | "STYLE" | "NOSCRIPT" => {}
                "BR" => out.push('\n'),
                "IMG" => {
                    let src = resolve(dom::attr(&child, "src").as_deref(), base);
                    let alt = dom::attr(&child, "alt").unwrap_or_default();
                    if !src.is_empty() {
                        out.push_str(&format!("![{}]({})", escape_brackets(&alt), src));
                    }
                }
                "A" => {
                    let text = inline_markdown(&child, base).trim().to_string();
                    let href = resolve(dom::attr(&child, "href").as_deref(), base);
                    if href.is_empty() {
                        out.push_str(&text);
                    } else {
                        out.push_str(&format!("[{}]({})", text, href));
                    }
                }
                "STRONG" | "B" => {
                    out.push_str(&format!("**{}**", inline_markdown(&child, base).trim()))
                }
                "EM" | "I" => out.push_str(&format!("*{}*", inline_markdown(&child, base).trim())),
                "DEL" | "S" | "STRIKE" => {
                    out.push_str(&format!("~~{}~~", inline_markdown(&child, base).trim()))
                }
                "CODE" => out.push_str(&format!("`{}`", dom::inner_text(&child, false).trim())),
                "IFRAME" | "VIDEO" => {
                    if let Some(src) = dom::attr(&child, "src") {
                        out.push_str(&format!("\n<iframe src=\"{}\"></iframe>\n", src));
                    }
                }
                _ => out.push_str(&inline_markdown(&child, base)),
            }
        }
    }
    out
}

fn normalize_inline(text: &str) -> String {
    if text.trim().is_empty() {
        return String::new();
    }
    // Collapse whitespace runs but keep the boundary single spaces, so
    // text nodes glue correctly around inline elements.
    let mut out = String::with_capacity(text.len());
    let mut last_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(c);
            last_space = false;
        }
    }
    out
}

fn escape_brackets(text: &str) -> String {
    text.replace('[', "\\[").replace(']', "\\]")
}

fn collapse_blank_lines(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    let mut blanks = 0;
    for line in md.lines() {
        if line.trim().is_empty() {
            blanks += 1;
            if blanks <= 2 {
                out.push('\n');
            }
        } else {
            blanks = 0;
            out.push_str(line);
            out.push('\n');
        }
    }
    out.trim().to_string()
}

/// Resolve `href` against the base URL (same rules as the reference
/// `make-links-absolute`). Absolute URLs pass through untouched.
pub fn resolve(href: Option<&str>, base: Option<&str>) -> String {
    let Some(href) = href else {
        return String::new();
    };
    let href = href.trim();
    if href.is_empty() || href.starts_with('#') || href.starts_with("data:") {
        return href.to_string();
    }
    let Some(base) = base else {
        return href.to_string();
    };
    match Url::parse(href) {
        Ok(u) => u.to_string(),
        Err(_) => Url::parse(base)
            .and_then(|b| b.join(href))
            .map(|u| u.to_string())
            .unwrap_or_else(|_| href.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_formatter_joins_paragraphs() {
        let dom = dom::parse(
            "<article><p>First para text.</p><p>Second <b>para</b> text.</p><a href='x'>link text</a><div></div></article>",
        );
        let article = dom::all_nodes_with_tag(&dom::document(&dom), &["ARTICLE"])[0].clone();
        let text = format_text(&article);
        assert!(text.contains("First para text."));
        assert!(text.contains("Second para text."));
        assert!(text.contains("link text"));
    }

    #[test]
    fn markdown_renders_headings_links_images() {
        let dom = dom::parse(
            "<article><h2>Heading</h2><p>Some <strong>bold</strong> text with a <a href='/go'>link</a>.</p><img src='/img/x.png' alt='pic'><ul><li>one</li><li>two</li></ul></article>",
        );
        let article = dom::all_nodes_with_tag(&dom::document(&dom), &["ARTICLE"])[0].clone();
        let md = to_markdown(&article, Some("https://example.com/post"));
        assert!(md.contains("## Heading"));
        assert!(md.contains("**bold**"));
        assert!(md.contains("[link](https://example.com/go)"));
        assert!(md.contains("![pic](https://example.com/img/x.png)"));
        assert!(md.contains("- one"));
        assert!(md.contains("- two"));
    }

    #[test]
    fn markdown_renders_tables() {
        let dom =
            dom::parse("<table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>");
        let table = dom::all_nodes_with_tag(&dom::document(&dom), &["TABLE"])[0].clone();
        let md = to_markdown(&table, None);
        assert!(md.contains("| A | B |"));
        assert!(md.contains("| --- | --- |"));
        assert!(md.contains("| 1 | 2 |"));
    }

    #[test]
    fn markdown_preserves_inline_spacing() {
        let dom = dom::parse("<article><p>Some <b>bold</b> body text.</p></article>");
        let article = dom::all_nodes_with_tag(&dom::document(&dom), &["ARTICLE"])[0].clone();
        let md = to_markdown(&article, None);
        assert!(md.contains("Some **bold** body"), "got: {md}");
    }

    #[test]
    fn nested_lists_do_not_duplicate_text() {
        let dom = dom::parse("<ul><li>parent item<ul><li>child item</li></ul></li></ul>");
        let ul = dom::all_nodes_with_tag(&dom::document(&dom), &["UL"])[0].clone();
        let md = to_markdown(&ul, None);
        assert_eq!(md.matches("child item").count(), 1, "got: {md}");
        assert_eq!(md.matches("parent item").count(), 1, "got: {md}");
    }

    #[test]
    fn resolve_relative_urls() {
        assert_eq!(
            resolve(Some("/img/x.png"), Some("https://example.com/post/1")),
            "https://example.com/img/x.png"
        );
        assert_eq!(
            resolve(Some("https://a.com/x"), Some("https://b.com")),
            "https://a.com/x"
        );
        assert_eq!(resolve(Some("#frag"), Some("https://b.com")), "#frag");
        assert_eq!(resolve(None, Some("https://b.com")), "");
    }
}
