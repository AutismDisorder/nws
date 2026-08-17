//! Port of readability's document pre/post-processing stages:
//! `_prepDocument`, `_replaceBrs`, `_unwrapNoscriptImages`, `_removeScripts`,
//! `_fixLazyImages`, `_postProcessContent` (`_fixRelativeUris`,
//! `_simplifyNestedElements`, `_cleanClasses`).

use crate::dom::{self, Handle};
use crate::regexes;

/// Classes readability keeps when stripping `class` attributes.
const CLASSES_TO_PRESERVE: &[&str] = &["page"];

// ------------------------------------------------------------ pre-document

/// Port of `_prepDocument`: remove `<style>`, collapse `<br>` chains into
/// `<p>`, re-tag `<font>` as `<span>`.
pub fn prep_document(doc: &Handle) {
    for s in dom::all_nodes_with_tag(doc, &["STYLE"]) {
        dom::detach(&s);
    }
    replace_brs(doc);
    for f in dom::all_nodes_with_tag(doc, &["FONT"]) {
        dom::set_tag(&f, "span");
    }
}

/// Port of `_replaceBrs`: collapse 2+ consecutive `<br>`s into a `<p>`
/// block, absorbing following phrasing content.
fn replace_brs(elem: &Handle) {
    let brs = dom::all_nodes_with_tag(elem, &["BR"]);
    for br in brs {
        let mut next = dom::next_sibling(&br);
        let mut replaced = false;
        loop {
            let mut n = next.clone();
            // readability `_nextNode` is element-only: whitespace text
            // between <br>s ("<br> <br>") must not hide the chain.
            loop {
                match &n {
                    Some(node) if dom::is_element(node) => break,
                    Some(node) => n = dom::next_sibling(node),
                    None => break,
                }
            }
            let Some(node) = n else { break };
            if dom::tag_is(&node, "BR") {
                replaced = true;
                let sibling = dom::next_sibling(&node);
                dom::detach(&node);
                next = sibling;
            } else {
                break;
            }
        }

        if replaced {
            let p = dom::create_element("p");
            dom::replace_node(&br, &p);
            let mut next = dom::next_sibling(&p);
            loop {
                let Some(node) = next.clone() else { break };
                if dom::tag_is(&node, "BR") {
                    let mut nn = dom::next_sibling(&node);
                    loop {
                        match &nn {
                            Some(n) if dom::is_element(n) => break,
                            Some(n) => nn = dom::next_sibling(n),
                            None => break,
                        }
                    }
                    if nn.as_ref().is_some_and(|n| dom::tag_is(n, "BR")) {
                        break;
                    }
                }
                if !dom::is_phrasing_content(&node) {
                    break;
                }
                let sibling = dom::next_sibling(&node);
                dom::append_child(&p, &node);
                next = sibling;
            }
            // Trim trailing whitespace nodes.
            loop {
                let Some(last) = dom::child_nodes(&p).last().cloned() else {
                    break;
                };
                if dom::is_whitespace_node(&last) {
                    dom::detach(&last);
                } else {
                    break;
                }
            }
            if let Some(parent) = dom::parent(&p) {
                if dom::tag_is(&parent, "P") {
                    dom::set_tag(&parent, "DIV");
                }
            }
        }
    }
}

/// Port of mercury's `convertLazyLoadedImages`: scan every attribute of
/// every `<img>` for link-looking values and promote them to real
/// `src`/`srcset` (including JSON-in-attribute values like
/// `data-src='{"src": "…"}'`).
pub fn convert_lazy_loaded_images(doc: &Handle) {
    for img in dom::all_nodes_with_tag(doc, &["IMG"]) {
        let attrs = dom::all_attrs(&img).unwrap_or_default();
        for (name, value) in &attrs {
            if name != "srcset"
                && regexes::is_link().is_match(value)
                && regexes::is_srcset().is_match(value)
            {
                dom::set_attr(&img, "srcset", value);
            } else if name != "src"
                && name != "srcset"
                && regexes::is_link().is_match(value)
                && regexes::is_image().is_match(value)
            {
                let src = extract_src_from_json(value).unwrap_or_else(|| value.clone());
                dom::set_attr(&img, "src", &src);
            }
        }
    }
}

/// `extractSrcFromJSON`: `{"src": "https://…"}` → the URL.
fn extract_src_from_json(value: &str) -> Option<String> {
    if !value.trim_start().starts_with('{') {
        return None;
    }
    let parsed: serde_json::Value = serde_json::from_str(value).ok()?;
    parsed.get("src")?.as_str().map(str::to_string)
}

/// Port of `_removeScripts`: drop `<script>` and `<noscript>`.
pub fn remove_scripts(doc: &Handle) {
    for n in dom::all_nodes_with_tag(doc, &["SCRIPT", "NOSCRIPT"]) {
        dom::detach(&n);
    }
}

/// Port of `_isSingleImage` over an already-parsed subtree.
fn is_single_image(node: &Handle) -> bool {
    let mut cur = node.clone();
    loop {
        if dom::tag_is(&cur, "IMG") {
            return true;
        }
        if dom::children(&cur).len() != 1 || !dom::text_content(&cur).trim().is_empty() {
            return false;
        }
        cur = dom::children(&cur)[0].clone();
    }
}

/// `<noscript>` content is raw text in the parser (scripting enabled), so
/// parse it as HTML and return the single image it contains, if any — the
/// equivalent of jsdom's `_isSingleImage(noscript)` + element extraction.
/// jsdom parses noscript content as elements, so the test is: exactly one
/// `<img>` and no text in the parsed fragment.
fn noscript_single_image(noscript: &Handle) -> Option<Handle> {
    let inner_html = dom::inner_text(noscript, false);
    let tmp_dom = dom::parse(&inner_html);
    let tmp_doc = dom::document(&tmp_dom);
    let imgs = dom::all_nodes_with_tag(&tmp_doc, &["IMG"]);
    if imgs.len() != 1 {
        return None;
    }
    if !dom::text_content(&tmp_doc).trim().is_empty() {
        return None;
    }
    imgs.into_iter().next()
}

fn has_image_attr(node: &Handle) -> bool {
    let Some(attrs) = dom::all_attrs(node) else {
        return false;
    };
    attrs.iter().any(|(name, value)| {
        matches!(name.as_str(), "src" | "srcset" | "data-src" | "data-srcset")
            || regexes::image_ext().is_match(value)
    })
}

/// Port of `_unwrapNoscriptImages`: replace placeholder images with the
/// real `<img>` hidden inside `<noscript>` (Medium-style lazy loading).
pub fn unwrap_noscript_images(doc: &Handle) {
    // 1. Remove `<img>` without any image-bearing attribute (placeholders).
    let imgs = dom::all_nodes_with_tag(doc, &["IMG"]);
    for img in imgs {
        if !has_image_attr(&img) {
            dom::detach(&img);
        }
    }

    // 2. Extract images from `<noscript>`.
    let noscripts = dom::all_nodes_with_tag(doc, &["NOSCRIPT"]);
    for noscript in noscripts {
        let Some(new_img) = noscript_single_image(&noscript) else {
            continue;
        };

        let prev_element = dom::previous_element_sibling(&noscript).filter(is_single_image);
        let Some(prev_element) = prev_element else {
            // No placeholder sibling: still swap the noscript for its image.
            let clone = dom::deep_clone(&new_img);
            dom::replace_node(&noscript, &clone);
            continue;
        };

        let prev_img = if dom::tag_is(&prev_element, "IMG") {
            prev_element.clone()
        } else {
            dom::all_nodes_with_tag(&prev_element, &["IMG"])
                .first()
                .cloned()
                .unwrap_or(prev_element.clone())
        };

        // Keep old (possibly useful) attributes on the new image.
        if let Some(attrs) = dom::all_attrs(&prev_img) {
            for (name, value) in attrs {
                if value.is_empty() {
                    continue;
                }
                if name == "src" || name == "srcset" || regexes::image_ext().is_match(&value) {
                    if dom::attr(&new_img, &name).as_deref() == Some(value.as_str()) {
                        continue;
                    }
                    let attr_name = if dom::has_attr(&new_img, &name) {
                        format!("data-old-{name}")
                    } else {
                        name.clone()
                    };
                    dom::set_attr(&new_img, &attr_name, &value);
                }
            }
        }

        let clone = dom::deep_clone(&new_img);
        dom::replace_node(&prev_element, &clone);
    }
}

// --------------------------------------------------------------- lazy imgs

/// Port of `_fixLazyImages`: convert `data-src`/`data-srcset`-style lazy
/// attributes into real `src`/`srcset`, drop tiny base64 placeholders.
pub fn fix_lazy_images(root: &Handle) {
    let nodes = dom::all_nodes_with_tag(root, &["IMG", "PICTURE", "FIGURE"]);
    for elem in nodes {
        // 1. Tiny base64 placeholder in src?
        if let Some(src) = dom::attr(&elem, "src") {
            if regexes::b64_data_url().is_match(&src) {
                let mime = regexes::b64_data_url()
                    .captures(&src)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str());
                if mime != Some("image/svg+xml") {
                    // Does another attribute carry a real image?
                    let src_could_be_removed = dom::all_attrs(&elem).is_some_and(|attrs| {
                        attrs.iter().any(|(name, value)| {
                            name != "src" && regexes::image_ext().is_match(value)
                        })
                    });
                    if src_could_be_removed {
                        let b64_len = src.len()
                            - regexes::b64_data_url()
                                .find(&src)
                                .map(|m| m.as_str().len())
                                .unwrap_or(0);
                        if b64_len < 133 {
                            dom::remove_attr(&elem, "src");
                        }
                    }
                }
            }
        }

        // 2. Skip if real src exists and the node doesn't look lazy.
        let class = dom::class_name(&elem).to_lowercase();
        let has_src = dom::attr(&elem, "src").is_some();
        let has_srcset = dom::attr(&elem, "srcset")
            .as_deref()
            .is_some_and(|s| s != "null");
        if (has_src || has_srcset) && !class.contains("lazy") {
            continue;
        }

        // 3. Promote image-looking attribute values.
        let attrs = dom::all_attrs(&elem).unwrap_or_default();
        for (name, value) in &attrs {
            if matches!(name.as_str(), "src" | "srcset" | "alt") {
                continue;
            }
            let copy_to = if regexes::img_srcset_attr().is_match(value) {
                Some("srcset")
            } else if regexes::img_src_attr().is_match(value) {
                Some("src")
            } else {
                None
            };
            if let Some(copy_to) = copy_to {
                if dom::tag_is(&elem, "IMG") || dom::tag_is(&elem, "PICTURE") {
                    dom::set_attr(&elem, copy_to, value);
                } else if dom::tag_is(&elem, "FIGURE")
                    && dom::all_nodes_with_tag(&elem, &["IMG", "PICTURE"]).is_empty()
                {
                    let img = dom::create_element("img");
                    dom::set_attr(&img, copy_to, value);
                    dom::append_child(&elem, &img);
                }
            }
        }
    }
}

// ------------------------------------------------------------- post-content

/// Port of `_postProcessContent`: absolutize URIs, simplify nested divs,
/// strip classes.
pub fn post_process_content(article: &Handle, base_url: Option<&str>, keep_classes: bool) {
    fix_relative_uris(article, base_url);
    simplify_nested_elements(article);
    if !keep_classes {
        clean_classes(article);
    }
}

/// Port of `_fixRelativeUris` + mercury's `make-links-absolute` `<base>`
/// preference: the `<base href>` of the page wins over the passed base.
pub fn fix_relative_uris(article: &Handle, base_uri: Option<&str>) {
    let base_uri = dom::all_nodes_with_tag(article, &["BASE"])
        .into_iter()
        .find_map(|b| dom::attr(&b, "href"))
        .or_else(|| base_uri.map(str::to_string));
    let to_absolute = |uri: &str| -> String {
        if uri.starts_with('#') {
            return uri.to_string();
        }
        let Some(base) = base_uri.as_deref() else {
            return uri.to_string();
        };
        match url::Url::parse(uri) {
            Ok(u) => u.to_string(),
            Err(_) => url::Url::parse(base)
                .and_then(|b| b.join(uri))
                .map(|u| u.to_string())
                .unwrap_or_else(|_| uri.to_string()),
        }
    };

    for link in dom::all_nodes_with_tag(article, &["A"]) {
        if let Some(href) = dom::attr(&link, "href") {
            if href.starts_with("javascript:") {
                // Scripts are gone; convert to inert content.
                let kids = dom::child_nodes(&link);
                if kids.len() == 1 && dom::is_text(&kids[0]) {
                    let text = dom::create_text(&dom::text_content(&kids[0]));
                    dom::replace_node(&link, &text);
                } else {
                    let span = dom::create_element("span");
                    for k in kids {
                        dom::append_child(&span, &k);
                    }
                    dom::replace_node(&link, &span);
                }
            } else {
                dom::set_attr(&link, "href", &to_absolute(&href));
            }
        }
    }

    for media in dom::all_nodes_with_tag(
        article,
        &["IMG", "PICTURE", "FIGURE", "VIDEO", "AUDIO", "SOURCE"],
    ) {
        if let Some(src) = dom::attr(&media, "src") {
            dom::set_attr(&media, "src", &to_absolute(&src));
        }
        if let Some(poster) = dom::attr(&media, "poster") {
            dom::set_attr(&media, "poster", &to_absolute(&poster));
        }
        if let Some(srcset) = dom::attr(&media, "srcset") {
            let new_srcset =
                regexes::srcset_url().replace_all(&srcset, |caps: &regex::Captures| {
                    let url = caps.get(1).map(|m| m.as_str()).unwrap_or("");
                    let rest = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                    let sep = caps.get(3).map(|m| m.as_str()).unwrap_or("");
                    format!("{}{}{}", to_absolute(url), rest, sep)
                });
            dom::set_attr(&media, "srcset", &new_srcset);
        }
    }
}

/// Port of `_removeAndGetNext` / `_simplifyNestedElements`.
fn simplify_nested_elements(article: &Handle) {
    let mut node = Some(article.clone());
    while let Some(n) = node {
        let should_merge = dom::parent(&n).is_some()
            && (dom::tag_is(&n, "DIV") || dom::tag_is(&n, "SECTION"))
            && !dom::attr(&n, "id").is_some_and(|id| id.starts_with("readability"));
        if should_merge {
            if dom::is_element_without_content(&n) {
                let next = dom::next_node(&n, true);
                dom::detach(&n);
                node = next;
                continue;
            } else if dom::has_single_tag_inside(&n, "DIV")
                || dom::has_single_tag_inside(&n, "SECTION")
            {
                let child = dom::children(&n)[0].clone();
                if let Some(attrs) = dom::all_attrs(&n) {
                    for (name, value) in attrs {
                        dom::set_attr(&child, &name, &value);
                    }
                }
                dom::replace_node(&n, &child);
                node = Some(child);
                continue;
            }
        }
        node = dom::next_node(&n, false);
    }
}

/// Port of `_cleanClasses`: keep only `CLASSES_TO_PRESERVE`.
fn clean_classes(node: &Handle) {
    if let Some(class) = dom::attr(node, "class") {
        let kept: Vec<&str> = class
            .split_whitespace()
            .filter(|c| CLASSES_TO_PRESERVE.contains(c))
            .collect();
        if kept.is_empty() {
            dom::remove_attr(node, "class");
        } else {
            dom::set_attr(node, "class", &kept.join(" "));
        }
    }
    for child in dom::children(node) {
        clean_classes(&child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_scripts_strips_script_and_noscript() {
        let dom = dom::parse(
            "<body><p>keep</p><script>evil()</script><noscript><img src='x.jpg'></noscript></body>",
        );
        let body = dom::all_nodes_with_tag(&dom::document(&dom), &["BODY"])[0].clone();
        remove_scripts(&body);
        assert!(dom::all_nodes_with_tag(&body, &["SCRIPT"]).is_empty());
        assert!(dom::all_nodes_with_tag(&body, &["NOSCRIPT"]).is_empty());
    }

    #[test]
    fn unwrap_noscript_replaces_placeholder() {
        let html = "<div><figure><img src='data:image/gif;base64,R0lGODlhAQABAAAAACw='></figure><noscript><img src='https://x.com/real.jpg' width='800'></noscript></div>";
        let dom = dom::parse(html);
        let doc = dom::document(&dom);
        unwrap_noscript_images(&doc);
        let imgs = dom::all_nodes_with_tag(&doc, &["IMG"]);
        let real = imgs
            .iter()
            .find(|i| dom::attr(i, "src").as_deref() == Some("https://x.com/real.jpg"));
        assert!(
            real.is_some(),
            "real noscript image should replace placeholder"
        );
    }

    #[test]
    fn fix_relative_uris_absolutizes_and_kills_js_links() {
        let dom = dom::parse(
            "<div><a href='/go'>go</a><a href='javascript:void(0)'>js</a><img src='img/x.png' srcset='img/a.png 1x, img/b.png 2x'></div>",
        );
        let div = dom::all_nodes_with_tag(&dom::document(&dom), &["DIV"])[0].clone();
        fix_relative_uris(&div, Some("https://example.com/post/1"));
        let links = dom::all_nodes_with_tag(&div, &["A"]);
        assert_eq!(
            dom::attr(&links[0], "href").as_deref(),
            Some("https://example.com/go")
        );
        assert!(
            dom::all_nodes_with_tag(&div, &["A"]).len() == 1,
            "javascript: link replaced by text"
        );
        let imgs = dom::all_nodes_with_tag(&div, &["IMG"]);
        assert_eq!(
            dom::attr(&imgs[0], "src").as_deref(),
            Some("https://example.com/post/img/x.png")
        );
        assert_eq!(
            dom::attr(&imgs[0], "srcset").as_deref(),
            Some("https://example.com/post/img/a.png 1x, https://example.com/post/img/b.png 2x")
        );
    }

    #[test]
    fn fix_lazy_images_promotes_data_src() {
        let dom = dom::parse(
            "<img src='data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP' data-src='https://x.com/big.jpg' class='lazyload'>",
        );
        let doc = dom::document(&dom);
        fix_lazy_images(&doc);
        let img = dom::all_nodes_with_tag(&doc, &["IMG"])[0].clone();
        assert_eq!(
            dom::attr(&img, "src").as_deref(),
            Some("https://x.com/big.jpg")
        );
    }

    #[test]
    fn clean_classes_keeps_only_preserved() {
        let dom = dom::parse("<div class='article foo page bar'><p class='x'>t</p></div>");
        let doc = dom::document(&dom);
        post_process_content(&doc, None, false);
        let div = dom::all_nodes_with_tag(&doc, &["DIV"])[0].clone();
        assert_eq!(dom::attr(&div, "class").as_deref(), Some("page"));
        let p = dom::all_nodes_with_tag(&doc, &["P"])[0].clone();
        assert_eq!(dom::attr(&p, "class"), None);
    }

    #[test]
    fn replace_brs_collapses_chains() {
        let dom = dom::parse("<body><p>before<br><br><br>after</p></body>");
        let body = dom::all_nodes_with_tag(&dom::document(&dom), &["BODY"])[0].clone();
        replace_brs(&body);
        let text = dom::inner_text(&body, true);
        assert!(text.contains("before"));
        assert!(text.contains("after"));
    }
}
