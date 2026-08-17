//! Minimal CSS selector matching over the mutable DOM — just enough for the
//! selector lists in mercury's generic extractors (tag, `.class`, `#id`,
//! `[attr]`, `[attr=v]`, `[attr*=v]`, `[attr^=v]`, descendant combinators).

use crate::dom::{self, Handle};

/// One compound selector: `tag#id.class[attr=v]`.
#[derive(Debug, Clone, Default)]
struct Compound {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    attrs: Vec<(String, String, char)>, // (name, value, op: '=' | '*' | '^')
}

fn parse_compound(s: &str) -> Option<Compound> {
    let mut c = Compound::default();
    let mut rest = s.trim().to_string();

    // attribute selectors: [name], [name=v], [name*=v], [name^=v]
    while let Some(open) = rest.find('[') {
        let close = rest[open..].find(']')? + open;
        let inner = &rest[open + 1..close];
        let (name, value, op) = if let Some(eq) = inner.find("^=") {
            (&inner[..eq], &inner[eq + 2..], '^')
        } else if let Some(eq) = inner.find("*=") {
            (&inner[..eq], &inner[eq + 2..], '*')
        } else if let Some(eq) = inner.find('=') {
            (&inner[..eq], &inner[eq + 1..], '=')
        } else {
            (inner, "", '\0')
        };
        c.attrs.push((
            name.trim().trim_matches('"').trim_matches('\'').to_string(),
            value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string(),
            op,
        ));
        rest = format!("{} {}", &rest[..open], &rest[close + 1..]);
    }

    let mut parts = rest.split_whitespace();
    let first = parts.next()?;
    // first part may be tag#id.class or #id.class or .class or tag
    let mut name = first.to_string();
    if let Some(hash) = name.find('#') {
        let (tag, rest_id) = name.split_at(hash);
        c.id = Some(rest_id[1..].split('.').next().unwrap_or("").to_string());
        name = tag.to_string();
    }
    // classes
    let remaining = name.clone();
    if let Some(dot) = remaining.find('.') {
        let (tag, classes) = remaining.split_at(dot);
        name = tag.to_string();
        for cls in classes.split('.') {
            if !cls.is_empty() {
                c.classes.push(cls.to_string());
            }
        }
    }
    if !name.is_empty() {
        c.tag = Some(name.to_lowercase());
    }
    Some(c)
}

fn match_compound(node: &Handle, c: &Compound) -> bool {
    if !dom::is_element(node) {
        return false;
    }
    if let Some(tag) = &c.tag {
        if dom::tag_name(node).as_deref() != Some(tag.to_uppercase().as_str()) {
            return false;
        }
    }
    if let Some(id) = &c.id {
        if dom::attr(node, "id").as_deref() != Some(id.as_str()) {
            return false;
        }
    }
    if !c.classes.is_empty() {
        let class = dom::class_name(node);
        let classes: Vec<&str> = class.split_whitespace().collect();
        if !c
            .classes
            .iter()
            .all(|want| classes.contains(&want.as_str()))
        {
            return false;
        }
    }
    for (name, value, op) in &c.attrs {
        let Some(v) = dom::attr(node, name) else {
            return false;
        };
        let ok = match op {
            '=' => &v == value,
            '*' => v.contains(value),
            '^' => v.starts_with(value),
            _ => true, // bare [attr]
        };
        if !ok {
            return false;
        }
    }
    true
}

/// Parse a selector with descendant combinators (`A B`) into compounds
/// in right-to-left matching order (last = the matched node).
fn parse_selector(selector: &str) -> Option<Vec<Compound>> {
    let mut compounds = Vec::new();
    for part in selector.split_whitespace() {
        if part == ">" {
            continue; // treat child combinator as descendant (close enough)
        }
        compounds.push(parse_compound(part)?);
    }
    if compounds.is_empty() {
        None
    } else {
        Some(compounds)
    }
}

/// Match a node against a full selector (descendant chain).
pub fn matches_selector(node: &Handle, selector: &str) -> bool {
    let Some(compounds) = parse_selector(selector) else {
        return false;
    };
    matches_compiled(node, &compounds)
}

/// Precompiled matching: walk the compound chain right-to-left (last
/// compound matches the node itself).
fn matches_compiled(node: &Handle, compounds: &[Compound]) -> bool {
    let mut cur = Some(node.clone());
    for c in compounds.iter().rev() {
        let Some(n) = cur else { return false };
        if !match_compound(&n, c) {
            return false;
        }
        cur = dom::parent(&n);
    }
    true
}

/// All elements under `root` matching `selector`, like `$(selector, root)`.
/// The selector is parsed once per query, not per element.
pub fn select_all(root: &Handle, selector: &str) -> Vec<Handle> {
    let Some(compounds) = parse_selector(selector) else {
        return Vec::new();
    };
    dom::all_elements(root)
        .into_iter()
        .filter(|n| matches_compiled(n, &compounds))
        .collect()
}

/// All elements in the whole document matching `selector`.
pub fn select_doc(doc: &Handle, selector: &str) -> Vec<Handle> {
    select_all(doc, selector)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_tag_id_class() {
        let dom =
            dom::parse("<html><body><h1 id='articleHeader' class='article x'>T</h1></body></html>");
        let doc = dom::document(&dom);
        let h1 = dom::all_nodes_with_tag(&doc, &["H1"])[0].clone();
        assert!(matches_selector(&h1, "h1#articleHeader"));
        assert!(matches_selector(&h1, "h1.article"));
        assert!(matches_selector(&h1, ".article"));
        assert!(!matches_selector(&h1, "h2"));
        assert!(!matches_selector(&h1, "h1.other"));
    }

    #[test]
    fn matches_descendant_and_attr() {
        let dom = dom::parse(
            "<html><body><div class='hentry'><h1 class='entry-title'>T</h1></div>\
             <a rel='author' href='/by/jane'>Jane</a></body></html>",
        );
        let doc = dom::document(&dom);
        assert_eq!(select_doc(&doc, ".hentry .entry-title").len(), 1);
        assert_eq!(select_doc(&doc, "a[rel=author]").len(), 1);
        assert_eq!(select_doc(&doc, "a[href*=author]").len(), 0);
        assert_eq!(select_doc(&doc, "a[href^=/by]").len(), 1);
    }
}
