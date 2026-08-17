//! Criterion benchmarks — single-doc extraction and parallel batches.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use nws::{extract, extract_many, output};

const PAGE: &str = r#"
<html lang="en"><head><title>Benchmark article | Bench Press</title>
<meta name="author" content="Ben Cher"></head><body>
<div class="article">
<h1>Benchmark article</h1>
<div class="byline">By Ben Cher</div>
<p>Paragraph number one contains a decent amount of text, with commas, clauses,
and enough characters to register in the scorer, as a real article would.</p>
<p>Paragraph number two continues in the same vein, adding more sentences,
more punctuation, and more of the substance that keeps the extraction
algorithm honest about its choices.</p>
<p>Paragraph number three is here to make the container big enough to beat
the five-hundred character threshold with room to spare, so the extractor
finishes on the first pass rather than degrading its heuristics.</p>
<p>Paragraph number four adds even more length, which matters for benchmarks
because realistic pages are long and the algorithm is linear in the number
of nodes, so we want a workload that resembles production.</p>
<p>Paragraph number five wraps up the body, and a footer follows to verify
that cleanup removes boilerplate without distorting the timing numbers.</p>
</div>
<div class="footer"><p>Copyright 2026 Bench Press, all rights reserved, legal
contact available on request, terms and conditions apply to everything.</p></div>
</body></html>
"#;

fn bench_extract(c: &mut Criterion) {
    c.benchmark_group("extract")
        .bench_function("single", |b| b.iter(|| extract(PAGE)));
}

fn bench_markdown(c: &mut Criterion) {
    // Markdown rendering on the cleaned tree, in isolation from parsing.
    let article = extract(PAGE).expect("extract");
    let dom = nws::dom::parse(&article.html);
    let tree = nws::dom::document(&dom);
    c.benchmark_group("output")
        .bench_function("to_markdown", |b| {
            b.iter(|| output::to_markdown(&tree, None))
        });
}

fn bench_extract_many(c: &mut Criterion) {
    let docs: Vec<&str> = (0..64).map(|_| PAGE).collect();
    let mut group = c.benchmark_group("extract_many");
    for n in [8usize, 32, 64] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter(|| extract_many(&docs[..n]))
        });
    }
}

criterion_group!(benches, bench_extract, bench_markdown, bench_extract_many);
criterion_main!(benches);
