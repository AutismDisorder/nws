//! Mutable-DOM helpers over `markup5ever_rcdom`.
//!
//! The readability algorithm constantly mutates the tree (removing nodes,
//! re-tagging, re-parenting), so we build on `RcDom` which exposes a mutable
//! `Rc<Node>` tree with parent links.

use html5ever::serialize::{serialize as serialize_html, SerializeOpts};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::{parse_document, Attribute};
use markup5ever::{ns, LocalName, QualName};
use markup5ever_rcdom::{Node, NodeData, RcDom, SerializableHandle};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

pub type NodeId = usize;

/// A node handle in the mutable tree.
pub type Handle = Rc<Node>;

#[inline]
pub fn id(node: &Handle) -> NodeId {
    Rc::as_ptr(node) as NodeId
}

/// Parse an HTML document into a mutable RcDom tree.
pub fn parse(html: &str) -> RcDom {
    parse_document(RcDom::default(), Default::default())
        .from_utf8()
        .one(html.as_bytes())
}

/// Document element (root) of the parsed tree.
pub fn document(dom: &RcDom) -> Handle {
    dom.document.clone()
}

// ---------------------------------------------------------------- creation

pub fn create_element(name: &str) -> Handle {
    Rc::new(Node {
        parent: Cell::new(None),
        children: RefCell::new(Vec::new()),
        data: NodeData::Element {
            name: QualName::new(None, ns!(html), LocalName::from(name.to_ascii_lowercase())),
            attrs: RefCell::new(Vec::new()),
            template_contents: RefCell::new(None),
            mathml_annotation_xml_integration_point: false,
        },
    })
}

pub fn create_text(text: &str) -> Handle {
    Rc::new(Node {
        parent: Cell::new(None),
        children: RefCell::new(Vec::new()),
        data: NodeData::Text {
            contents: RefCell::new(StrTendril::from(text)),
        },
    })
}

// ---------------------------------------------------------------- basics

pub fn is_element(node: &Handle) -> bool {
    matches!(node.data, NodeData::Element { .. })
}

pub fn is_text(node: &Handle) -> bool {
    matches!(node.data, NodeData::Text { .. })
}

pub fn is_comment(node: &Handle) -> bool {
    matches!(node.data, NodeData::Comment { .. })
}

/// Uppercased tag name (readability compares against uppercase HTML tags).
pub fn tag_name(node: &Handle) -> Option<String> {
    match &node.data {
        NodeData::Element { name, .. } => Some(name.local.to_string().to_uppercase()),
        _ => None,
    }
}

pub fn tag_is(node: &Handle, tag: &str) -> bool {
    matches!(tag_name(node).as_deref(), Some(t) if t == tag)
}

/// The `class` attribute value ("" when absent), like `node.className` in JS.
pub fn class_name(node: &Handle) -> String {
    attr(node, "class").unwrap_or_default()
}

/// `className + " " + id` — the string readability regexes run against.
pub fn match_string(node: &Handle) -> String {
    let class = class_name(node);
    let id = attr(node, "id").unwrap_or_default();
    if id.is_empty() {
        class
    } else {
        format!("{class} {id}")
    }
}

pub fn attr(node: &Handle, name: &str) -> Option<String> {
    match &node.data {
        NodeData::Element { attrs, .. } => attrs
            .borrow()
            .iter()
            .find(|a| a.name.local.to_string().eq_ignore_ascii_case(name))
            .map(|a| a.value.to_string()),
        _ => None,
    }
}

pub fn set_attr(node: &Handle, name: &str, value: &str) {
    if let NodeData::Element { attrs, .. } = &node.data {
        let mut attrs = attrs.borrow_mut();
        if let Some(a) = attrs
            .iter_mut()
            .find(|a| a.name.local.to_string().eq_ignore_ascii_case(name))
        {
            a.value = StrTendril::from(value);
        } else {
            attrs.push(Attribute {
                name: QualName::new(None, ns!(), LocalName::from(name.to_ascii_lowercase())),
                value: StrTendril::from(value),
            });
        }
    }
}

pub fn remove_attr(node: &Handle, name: &str) {
    if let NodeData::Element { attrs, .. } = &node.data {
        attrs
            .borrow_mut()
            .retain(|a| !a.name.local.to_string().eq_ignore_ascii_case(name));
    }
}

pub fn has_attr(node: &Handle, name: &str) -> bool {
    attr(node, name).is_some()
}

/// All attributes as (name, value) pairs.
pub fn all_attrs(node: &Handle) -> Option<Vec<(String, String)>> {
    match &node.data {
        NodeData::Element { attrs, .. } => Some(
            attrs
                .borrow()
                .iter()
                .map(|a| (a.name.local.to_string(), a.value.to_string()))
                .collect(),
        ),
        _ => None,
    }
}

// ---------------------------------------------------------------- tree walks

pub fn parent(node: &Handle) -> Option<Handle> {
    let w = node.parent.take();
    let p = w.as_ref().and_then(Weak::upgrade);
    node.parent.set(w);
    p
}

/// All child nodes (including text), snapshot.
pub fn child_nodes(node: &Handle) -> Vec<Handle> {
    node.children.borrow().clone()
}

/// Element children only (JS `node.children`).
pub fn children(node: &Handle) -> Vec<Handle> {
    child_nodes(node).into_iter().filter(is_element).collect()
}

pub fn first_child(node: &Handle) -> Option<Handle> {
    node.children.borrow().first().cloned()
}

pub fn first_element_child(node: &Handle) -> Option<Handle> {
    node.children
        .borrow()
        .iter()
        .find(|c| is_element(c))
        .cloned()
}

pub fn last_element_child(node: &Handle) -> Option<Handle> {
    node.children
        .borrow()
        .iter()
        .rev()
        .find(|c| is_element(c))
        .cloned()
}

pub fn next_sibling(node: &Handle) -> Option<Handle> {
    let p = parent(node)?;
    let children = p.children.borrow();
    let pos = children.iter().position(|c| Rc::ptr_eq(c, node))?;
    children.get(pos + 1).cloned()
}

/// Previous sibling node (any node type).
pub fn previous_sibling(node: &Handle) -> Option<Handle> {
    let p = parent(node)?;
    let children = p.children.borrow();
    let pos = children.iter().position(|c| Rc::ptr_eq(c, node))?;
    if pos == 0 {
        None
    } else {
        children.get(pos - 1).cloned()
    }
}

pub fn next_element_sibling(node: &Handle) -> Option<Handle> {
    let mut cur = next_sibling(node)?;
    loop {
        if is_element(&cur) {
            return Some(cur);
        }
        cur = next_sibling(&cur)?;
    }
}

/// Previous element sibling (JS `previousElementSibling`).
pub fn previous_element_sibling(node: &Handle) -> Option<Handle> {
    let p = parent(node)?;
    let kids = p.children.borrow();
    let pos = kids.iter().position(|c| Rc::ptr_eq(c, node))?;
    kids[..pos].iter().rev().find(|c| is_element(c)).cloned()
}

/// Port of readability's `_getNextNode` — element-only depth-first traversal.
pub fn next_node(node: &Handle, ignore_self_and_kids: bool) -> Option<Handle> {
    if !ignore_self_and_kids {
        if let Some(fc) = first_element_child(node) {
            return Some(fc);
        }
    }
    if let Some(ns) = next_element_sibling(node) {
        return Some(ns);
    }
    let mut cur = node.clone();
    loop {
        let p = parent(&cur)?;
        if let Some(ns) = next_element_sibling(&p) {
            return Some(ns);
        }
        cur = p;
    }
}

/// All element descendants (DFS), including `root` when it is an element.
pub fn all_elements(root: &Handle) -> Vec<Handle> {
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(n) = stack.pop() {
        if is_element(&n) {
            out.push(n.clone());
        }
        let kids = n.children.borrow();
        for c in kids.iter().rev() {
            stack.push(c.clone());
        }
    }
    out
}

/// All descendant *nodes* (elements, text, comments), DFS.
pub fn all_nodes(root: &Handle) -> Vec<Handle> {
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(n) = stack.pop() {
        out.push(n.clone());
        let kids = n.children.borrow();
        for c in kids.iter().rev() {
            stack.push(c.clone());
        }
    }
    out
}

/// Port of `getElementsByTagName(tag)` for a tag set.
pub fn all_nodes_with_tag(root: &Handle, tags: &[&str]) -> Vec<Handle> {
    all_elements(root)
        .into_iter()
        .filter(|e| tag_name(e).as_deref().is_some_and(|t| tags.contains(&t)))
        .collect()
}

/// Port of newspaper's `getElementsByTag(doc, attr=..., value=...)`:
/// elements carrying an attribute named `attr` with exactly `value`.
pub fn elements_with_attr_value(root: &Handle, attr_name: &str, value: &str) -> Vec<Handle> {
    all_elements(root)
        .into_iter()
        .filter(|e| attr(e, attr_name).as_deref() == Some(value))
        .collect()
}

// ---------------------------------------------------------------- mutation

/// Raw outer HTML for a hoisted `<a>` node (port of `outerHtml` used by
/// `replace_walk_left_right`) — unescaped, so re-parsing yields elements.
pub fn link_outer_html(node: &Handle) -> String {
    let mut attrs = String::new();
    if let Some(all) = all_attrs(node) {
        for (name, value) in all {
            attrs.push_str(&format!(" {}=\"{}\"", name, value.replace('"', "&quot;")));
        }
    }
    format!("<a{}>{}</a>", attrs, text_content(node))
}

/// Remove `node` from its parent. Safe to call on detached nodes.
pub fn detach(node: &Handle) {
    let w = node.parent.take();
    if let Some(w) = w {
        if let Some(p) = w.upgrade() {
            p.children.borrow_mut().retain(|c| !Rc::ptr_eq(c, node));
        }
    }
}

pub fn append_child(parent: &Handle, child: &Handle) {
    detach(child);
    child.parent.set(Some(Rc::downgrade(parent)));
    parent.children.borrow_mut().push(child.clone());
}

/// Insert `new_node` into `parent` before `reference` (which must be a child).
pub fn insert_before(parent: &Handle, new_node: &Handle, reference: &Handle) {
    detach(new_node);
    let mut kids = parent.children.borrow_mut();
    if let Some(pos) = kids.iter().position(|c| Rc::ptr_eq(c, reference)) {
        new_node.parent.set(Some(Rc::downgrade(parent)));
        kids.insert(pos, new_node.clone());
    }
}

/// Port of `_setNodeTag`: replace a node with a fresh element of `new_tag`,
/// moving all children and attributes over. Returns the replacement.
pub fn set_tag(node: &Handle, new_tag: &str) -> Handle {
    let replacement = create_element(new_tag);
    if let Some(p) = parent(node) {
        insert_before(&p, &replacement, node);
    }
    // Move children.
    let kids = node.children.replace(Vec::new());
    for k in kids {
        append_child(&replacement, &k);
    }
    // Copy attributes.
    if let NodeData::Element { attrs, .. } = &node.data {
        if let NodeData::Element {
            attrs: new_attrs, ..
        } = &replacement.data
        {
            *new_attrs.borrow_mut() = attrs.borrow().clone();
        }
    }
    detach(node);
    replacement
}

/// Unwrap a node in place: replace it with its children, keeping the
/// contents (port of lxml's `drop_tag`).
pub fn unwrap(node: &Handle) {
    let Some(parent) = parent(node) else {
        return;
    };
    let kids = child_nodes(node);
    for k in &kids {
        detach(k);
    }
    for k in &kids {
        insert_before(&parent, k, node);
    }
    detach(node);
}

/// Port of mercury's `normalizeMetaTags`: `<meta content>` → `value` and
/// `property` → `name`, so the custom extractors read `value` uniformly.
/// Run after meta collection (which prefers the original attributes).
pub fn normalize_meta_tags(doc: &Handle) {
    for el in all_nodes_with_tag(doc, &["META"]) {
        if let Some(v) = attr(&el, "content") {
            set_attr(&el, "value", &v);
            remove_attr(&el, "content");
        }
        if let Some(v) = attr(&el, "property") {
            set_attr(&el, "name", &v);
            remove_attr(&el, "property");
        }
    }
}

/// Deep-clone a node subtree into fresh handles (no parent links),
/// used to transplant nodes parsed from `<noscript>` markup.
pub fn deep_clone(node: &Handle) -> Handle {
    match &node.data {
        NodeData::Text { contents } => create_text(&contents.borrow()),
        NodeData::Element { name, attrs, .. } => {
            let clone = create_element(name.local.as_ref());
            if let NodeData::Element {
                attrs: new_attrs, ..
            } = &clone.data
            {
                *new_attrs.borrow_mut() = attrs.borrow().clone();
            }
            for child in node.children.borrow().iter() {
                let c = deep_clone(child);
                append_child(&clone, &c);
            }
            clone
        }
        _ => create_text(""),
    }
}

/// Replace `node` with `replacement` (moves nothing else).
pub fn replace_node(node: &Handle, replacement: &Handle) {
    if let Some(p) = parent(node) {
        insert_before(&p, replacement, node);
    }
    detach(node);
}

// ---------------------------------------------------------------- text

/// Port of JS `textContent`: all descendant text, concatenated.
pub fn text_content(node: &Handle) -> String {
    let mut s = String::new();
    fn walk(n: &Handle, s: &mut String) {
        match &n.data {
            NodeData::Text { contents } => s.push_str(&contents.borrow()),
            NodeData::Element { .. } | NodeData::Document => {
                for c in n.children.borrow().iter() {
                    walk(c, s);
                }
            }
            _ => {}
        }
    }
    walk(node, &mut s);
    s
}

/// Port of readability's `_getInnerText`: trimmed, optionally
/// whitespace-collapsed, text.
pub fn inner_text(node: &Handle, normalize_spaces: bool) -> String {
    let t = text_content(node);
    let t = t.trim();
    if normalize_spaces {
        crate::regexes::normalize().replace_all(t, " ").into_owned()
    } else {
        t.to_string()
    }
}

/// Serialize a subtree back to HTML.
pub fn serialize(node: &Handle) -> String {
    let mut buf = Vec::new();
    let _ = serialize_html(
        &mut buf,
        &SerializableHandle::from(node.clone()),
        SerializeOpts::default(),
    );
    let s = String::from_utf8_lossy(&buf).into_owned();
    if s.is_empty() && is_element(node) {
        // html5ever yields nothing when serializing a childless element
        // directly; inside a parent it serializes fine. Wrap, serialize,
        // strip the wrapper.
        let wrapper = create_element("div");
        let clone = deep_clone(node);
        append_child(&wrapper, &clone);
        let wrapped = serialize(&wrapper);
        let inner = wrapped
            .strip_prefix("<div>")
            .and_then(|w| w.strip_suffix("</div>"))
            .unwrap_or(&wrapped);
        return inner.to_string();
    }
    s
}

// ---------------------------------------------------------------- misc checks

/// Port of `_isProbablyVisible` (approximation: no computed styles available
/// outside a browser, so we inspect inline styles + hidden/aria-hidden).
pub fn is_probably_visible(node: &Handle) -> bool {
    let style = attr(node, "style").unwrap_or_default().to_lowercase();
    // Tolerate spacing/casing inside declarations: "visibility: hidden".
    let style_nospace: String = style.chars().filter(|c| !c.is_whitespace()).collect();
    if style_nospace.contains("display:none") || style_nospace.contains("visibility:hidden") {
        return false;
    }
    if has_attr(node, "hidden") {
        return false;
    }
    if let Some(v) = attr(node, "aria-hidden") {
        if v == "true" && !class_name(node).contains("fallback-image") {
            return false;
        }
    }
    true
}

/// Port of `_hasAncestorTag`.
pub fn has_ancestor_tag(node: &Handle, tag: &str) -> bool {
    let mut cur = node.clone();
    let mut depth = 0;
    while let Some(p) = parent(&cur) {
        if depth > 3 {
            return false;
        }
        if tag_is(&p, tag) {
            return true;
        }
        cur = p;
        depth += 1;
    }
    false
}

/// Port of `_hasSingleTagInsideElement`.
pub fn has_single_tag_inside(element: &Handle, tag: &str) -> bool {
    let elems = children(element);
    if elems.len() != 1 || !tag_is(&elems[0], tag) {
        return false;
    }
    for n in child_nodes(element) {
        if is_text(&n) && crate::regexes::has_content().is_match(&text_content(&n)) {
            return false;
        }
    }
    true
}

/// Port of `_isElementWithoutContent`.
pub fn is_element_without_content(node: &Handle) -> bool {
    if !is_element(node) {
        return false;
    }
    if !text_content(node).trim().is_empty() {
        return false;
    }
    let kids = children(node);
    let br_hr = kids
        .iter()
        .filter(|k| tag_is(k, "BR") || tag_is(k, "HR"))
        .count();
    kids.is_empty() || kids.len() == br_hr
}

const DIV_TO_P_ELEMS: &[&str] = &[
    "BLOCKQUOTE",
    "DL",
    "DIV",
    "IMG",
    "OL",
    "P",
    "PRE",
    "TABLE",
    "UL",
];

/// Port of `_hasChildBlockElement`.
pub fn has_child_block_element(element: &Handle) -> bool {
    fn some_block(n: &Handle) -> bool {
        let tag = tag_name(n);
        if let Some(t) = tag.as_deref() {
            if DIV_TO_P_ELEMS.contains(&t) {
                return true;
            }
        }
        for c in n.children.borrow().iter() {
            if some_block(c) {
                return true;
            }
        }
        false
    }
    for c in child_nodes(element) {
        if some_block(&c) {
            return true;
        }
    }
    false
}

const PHRASING_ELEMS: &[&str] = &[
    "ABBR", "AUDIO", "B", "BDO", "BR", "BUTTON", "CITE", "CODE", "DATA", "DATALIST", "DFN", "EM",
    "EMBED", "I", "IMG", "INPUT", "KBD", "LABEL", "MARK", "MATH", "METER", "NOSCRIPT", "OBJECT",
    "OUTPUT", "PROGRESS", "Q", "RUBY", "SAMP", "SCRIPT", "SELECT", "SMALL", "SPAN", "STRONG",
    "SUB", "SUP", "TEXTAREA", "TIME", "VAR", "WBR",
];

/// Port of `_isPhrasingContent`.
pub fn is_phrasing_content(node: &Handle) -> bool {
    if is_text(node) {
        return true;
    }
    let Some(tag) = tag_name(node) else {
        return false;
    };
    if PHRASING_ELEMS.contains(&tag.as_str()) {
        return true;
    }
    if tag == "A" || tag == "DEL" || tag == "INS" {
        return child_nodes(node).iter().all(is_phrasing_content);
    }
    false
}

/// Port of `_isWhitespace`.
pub fn is_whitespace_node(node: &Handle) -> bool {
    (is_text(node) && text_content(node).trim().is_empty())
        || (is_element(node) && tag_is(node, "BR"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_walk() {
        let dom = parse("<html><body><div id='a'><p>x</p></div><p>y</p></body></html>");
        let doc = document(&dom);
        let ps = all_nodes_with_tag(&doc, &["P"]);
        assert_eq!(ps.len(), 2);
        let div = elements_with_attr_value(&doc, "id", "a");
        assert_eq!(div.len(), 1);
        assert_eq!(inner_text(&div[0], true), "x");
    }

    #[test]
    fn detach_and_set_tag() {
        let dom = parse("<html><body><div><p>hi</p></div></body></html>");
        let doc = document(&dom);
        let p = all_nodes_with_tag(&doc, &["P"])[0].clone();
        detach(&p);
        assert_eq!(all_nodes_with_tag(&doc, &["P"]).len(), 0);
        let div = all_nodes_with_tag(&doc, &["DIV"])[0].clone();
        append_child(&div, &p);
        let replacement = set_tag(&p, "SPAN");
        assert_eq!(all_nodes_with_tag(&doc, &["SPAN"]).len(), 1);
        assert_eq!(text_content(&replacement), "hi");
    }

    #[test]
    fn serialize_roundtrip() {
        let dom = parse("<html><body><p>hello world</p></body></html>");
        let s = serialize(&document(&dom));
        assert!(s.contains("hello world"));
    }
}

#[cfg(test)]
mod roundtrip_tests {
    use super::*;

    #[test]
    fn serialize_parse_roundtrip_stable() {
        // serialize → parse → serialize must converge.
        let html =
            "<div id='a' class='b c'><p>x <b>y</b> z</p><img src='i.png' alt='a&amp;b'></div>";
        let dom1 = parse(html);
        let s1 = serialize(&document(&dom1));
        let dom2 = parse(&s1);
        let s2 = serialize(&document(&dom2));
        assert_eq!(s1, s2, "serialization must be a fixpoint");
        assert!(s2.contains("<img") && s2.contains("a&amp;b"));
    }
}
