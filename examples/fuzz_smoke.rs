//! Deterministic fuzz smoke: mutate real-page seeds (byte flips, truncations,
//! duplications, injections) plus pathological inputs, and run the full
//! pipeline inside `catch_unwind` to prove no input panics.
//!
//! Run: `cargo run --release --example fuzz_smoke -- <iterations>`

fn main() {
    let iters: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(20_000);

    let seeds: Vec<&str> = vec![
        include_str!("../reference/readability/test/test-pages/001/source.html"),
        include_str!("../reference/readability/test/test-pages/aclu/source.html"),
        "<html><head><title>t</title></head><body><article><h1>t</h1><p>text text text text text text text text text text text text</p></article></body></html>",
        "<div><p>one</p><a href='/x'>link</a><img src='x.png'><table><tr><td>cell</td></tr></table></div>",
        "",
        "<p>",
        "<!-- comment only -->",
    ];

    let pathological: Vec<String> = vec![
        "<div>".repeat(10_000) + &"</div>".repeat(10_000),
        "<p>".repeat(50_000),
        format!("<p title='{}'>x</p>", "a".repeat(100_000)),
        "a".repeat(1_000_000),
        "\u{0}\u{0}\u{0}".repeat(10_000),
        "<img src=\"data:image/gif;base64,AAAA\" srcset='x 1w, y 2w'>".to_string(),
        "<a href='javascript:void(0)'><span>nested</span></a>".to_string(),
        "<noscript><img src='x.jpg'></noscript><figure><img src='data:image/gif;base64,R0lGODlhAQAB'></figure>".to_string(),
        "<table><tbody><tr><td><p>x</p></td></tr></tbody></table>".to_string(),
        "<html><head><script type='application/ld+json'>{}</script></head><body></body></html>".to_string(),
    ];

    let mut rng = XorShift(0x9E3779B97F4A7C15);

    let mut panics = 0u64;
    let mut errors = 0u64;
    let mut ok = 0u64;

    // True when the pipeline panicked.
    fn run_one(html: &str) -> bool {
        std::panic::catch_unwind(|| {
            let _ = nws::extract(html);
            let _ = nws::readerable::is_readerable_html(html);
            let _ = nws::extract_many(&[html, html]);
            #[cfg(feature = "http")]
            let _ = nws::fetch::FetchOptions::default();
            // Full output path: markdown + html on the raw tree too.
            if let Ok(a) = nws::extract(html) {
                let dom = nws::dom::parse(&a.html);
                let tree = nws::dom::document(&dom);
                let _ = nws::output::to_markdown(&tree, Some("https://example.com/x"));
                let _ = nws::output::format_text(&tree);
                let _ = nws::videos::get_videos(&tree);
                let _ = nws::nlp::keywords(&a.text);
                let _ = nws::nlp::summarize(&a.title, &a.text, 3);
                let _ = serde_json::to_string(&a);
            }
        })
        .is_err()
    }

    // Pathological + seeds first.
    for p in &pathological {
        if run_one(p) {
            panics += 1;
        } else {
            ok += 1;
        }
    }
    for s in &seeds {
        if run_one(s) {
            panics += 1;
        } else {
            ok += 1;
        }
    }

    // Mutations.
    for i in 0..iters {
        let seed = seeds[(rng.next() as usize) % seeds.len()].as_bytes();
        if seed.is_empty() {
            if run_one("") {
                panics += 1;
            } else {
                ok += 1;
            }
            continue;
        }
        let mut v: Vec<u8> = seed.to_vec();
        let ops = 1 + (rng.next() as usize % 4);
        for _ in 0..ops {
            match rng.next() % 5 {
                0 => {
                    // byte flip
                    let pos = rng.next() as usize % v.len();
                    v[pos] ^= (rng.next() as u8) | 1;
                }
                1 => {
                    // truncate
                    let pos = rng.next() as usize % v.len();
                    v.truncate(pos.max(1));
                }
                2 => {
                    // duplicate a slice
                    let a = rng.next() as usize % v.len();
                    let b = a + (rng.next() as usize % (v.len() - a));
                    let dup = v[a..b].to_vec();
                    let pos = rng.next() as usize % v.len();
                    v.splice(pos..pos, dup);
                }
                3 => {
                    // insert a byte
                    let pos = rng.next() as usize % v.len();
                    v.insert(pos, (rng.next() % 256) as u8);
                }
                _ => {
                    // inject a tag fragment
                    let frags: &[&[u8]] =
                        &[b"<div>", b"</div>", b"<p", b"\"", b"&amp;", b"<a href=''>"];
                    let f = frags[(rng.next() as usize) % frags.len()];
                    let pos = rng.next() as usize % v.len();
                    v.splice(pos..pos, f.iter().copied());
                }
            }
        }
        match std::str::from_utf8(&v) {
            Ok(s) => {
                if run_one(s) {
                    panics += 1;
                } else {
                    ok += 1;
                }
            }
            Err(_) => {
                // invalid UTF-8 is rejected by the API surface — only count
                // panics for it, never call extract.
                errors += 1;
            }
        }
        if i % 5_000 == 0 && i > 0 {
            eprintln!("… {i}/{iters} done (panics so far: {panics})");
        }
    }

    println!("iterations: {iters}");
    println!("ok: {ok}, parse-rejected (invalid utf8): {errors}, PANICS: {panics}");
    if panics > 0 {
        std::process::exit(1);
    }
    println!("no panics");
}

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}
