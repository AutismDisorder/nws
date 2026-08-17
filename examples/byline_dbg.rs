fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/bbc.html".to_string());
    let html = std::fs::read_to_string(&path).expect("read input");
    let dom = nws::dom::parse(&html);
    let doc = nws::dom::document(&dom);
    let attrs = ["name", "rel", "itemprop", "class", "id"];
    let vals = ["author", "byline", "dc.creator", "byl"];
    for a in attrs {
        for v in vals {
            for el in nws::dom::elements_with_attr_value(&doc, a, v) {
                let content = if nws::dom::tag_is(&el, "META") {
                    nws::dom::attr(&el, "content").unwrap_or_default()
                } else {
                    nws::dom::text_content(&el)
                };
                let tag = nws::dom::tag_name(&el).unwrap_or_default();
                let preview: String = content.chars().take(120).collect();
                eprintln!(
                    "[{a}={v}] <{tag}> content({}): {:?}",
                    content.len(),
                    preview
                );
            }
        }
    }
}
