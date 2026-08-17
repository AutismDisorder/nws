fn main() {
    let html = std::fs::read_to_string("/tmp/bbc-nws.html").unwrap();
    let dom = nws::dom::parse(&html);
    let doc = nws::dom::document(&dom);
    nws::post::unwrap_noscript_images(&doc);
    nws::post::convert_lazy_loaded_images(&doc);
    nws::post::remove_scripts(&doc);
    nws::dom::normalize_meta_tags(&doc);
    nws::post::prep_document(&doc);
    let mut g = nws::grab::Grabber::new(nws::dom::serialize(&doc), 500, 5);
    let _ = g.grab_article();
    eprintln!("grab byline: {:?}", g.article_byline);
    eprintln!(
        "meta::authors(doc, byline): {:?}",
        nws::meta::authors(&doc, g.article_byline.as_deref())
    );
    let a = nws::extract(&html).unwrap();
    eprintln!("final authors: {:?}", a.authors);
}
