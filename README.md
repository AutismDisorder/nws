# nws

**Article extraction engine** — the `newspaper3k` / `mercury-parser` /
`mozilla/readability` lineage rebuilt in Rust, heavily optimized.

Feed it an HTML page; get back a structured article:

- **title** — newspaper3k's `get_title` (h1 + og:title comparisons, separator
  splitting) + readability's JSON-LD (`headline`/`name` similarity tiebreak)
- **markdown** — the article body as clean Markdown for LLM/RAG ingestion
- **text / html** — readability's scoring algorithm (clean body, no nav/footer/ads)
- **authors** — newspaper3k's byline tokenizer + readability's byline + JSON-LD
- **publish_date** — URL regex → meta tags → `<time>` → JSON-LD → byline text
- **top_image** — meta short-circuit, then mercury's scored image selection
  (URL hints, alt, figure parents, figcaption sibling, dimensions, position)
- **images / favicon / canonical / site_name / description / dek / keywords / language**
- **excerpt / word_count / summary / next_page / videos**

## Usage

```rust
use nws::extract;

let article = extract(&html)?;
println!("{} — {}", article.title, article.authors.join(", "));
println!("{}", article.text);
```

```rust
// Parallel batch extraction (rayon):
let results: Vec<nws::Result<nws::Article>> = nws::extract_many(&docs);
```

```rust
// Async fetch + extract (tokio + reqwest, feature "http"):
let article = nws::fetch::extract_url("https://example.com/story").await?;
```

CLI:

```text
$ nws https://blog.rust-lang.org/2026/07/16/Rust-1.97.1/
{
  "title": "Announcing Rust 1.97.1",
  "authors": ["The Rust Release Team"],
  "publish_date": "2026-07-16",
  ...
}
$ nws --markdown <url>     # just the Markdown body
```

HTTP API (feature `server`):

```text
$ cargo run --release --features server --bin nws-server
GET  /health             -> {"status":"ok", …}
GET  /extract?url=<url>  -> JSON article (Accept: text/markdown | text/html)
POST /extract            -> {"url": …} or {"html": …}
POST /batch              -> {"urls":[…] } or {"docs":[{"html":…}, …]} (fetch: tokio, 16 concurrent; extract: rayon, all cores)
```

## Performance

Measured on this machine (criterion, release profile, synthetic 5-paragraph page):

| workload          | time      | throughput      |
|-------------------|-----------|-----------------|
| single page       | ~580 µs   | ~1,700 pages/s  |
| 64-page batch     | ~16.9 ms  | ~3,800 pages/s  |

(single-page includes the full pipeline: parse, score, clean, JSON-LD, NLP
keywords + summary, Markdown rendering.)

- Parsing: html5ever (browser-grade HTML5 parser, 30M+ downloads/90 days).
- Parallelism: `rayon` for batch extraction across all cores; CPU work runs
  on the blocking pool so the async executor stays free for I/O.
- Async: `tokio` + `reqwest` (rustls) with one shared connection/TLS pool per
  (timeout, user-agent); batch URL fetches run concurrently (16-way bounded).
- Network hygiene: 10 MiB `MAX_HTML_BYTES` page cap (content-length pre-check
  + post-read check); image dimension checks read only the first 64 KiB chunk
  and abort — headers, not pixels.
- No GC, no GIL, one static binary per platform — the cheapest possible COGS
  for a hosted extraction API.

## Design

A faithful port of the reference algorithm, with the DOM made mutable
(`markup5ever_rcdom` arena with parent links) because the algorithm
constantly re-tags, detaches, and re-parents nodes:

- `grab.rs` — `_grabArticle`: paragraph scoring, class/id weighting, top-candidate
  selection, sibling expansion, flag-degradation retries (`char_threshold`).
- `clean.rs` — `_prepArticle`/`_cleanConditionally`: data tables, share elements,
  embeds, headers, single-cell table flattening.
- `meta.rs` — newspaper3k metadata strategies (title/authors/date/image/lang)
  + readability JSON-LD parsing.
- `post.rs` — `_prepDocument`, noscript image swap, lazy-image (`data-src`) fix,
  relative-URI absolutization, nested-element simplification, class stripping.
- `cleaners.rs` — newspaper3k `DocumentCleaner` (naughty tags, drop caps,
  `div_to_para` with link hoisting).
- `output.rs` — newspaper output formatter + Markdown renderer.
- `nlp.rs` — newspaper keywords/summarize (stopword port, no external NLP deps).
- `images.rs` / `pagination.rs` — mercury lead-image and next-page scoring.
- `videos.rs` — newspaper video embed extraction (YouTube/Vimeo/Dailymotion/Kewego).
- `score.rs` — link density, text density, class weight, text similarity.

Known limitations: visibility checks are exactly the references' — inline
`style`, `hidden`, `aria-hidden` (readability's `_isProbablyVisible`); none of
the three originals compute stylesheet-based visibility either, since they run
on jsdom/cheerio/lxml, not browsers. A headless-Chromium pass would be an
enhancement *beyond* the references for CSS-hidden content. Stopword files
beyond English can be dropped into `src/` and wired through `nlp.rs`.

## Licensing

**AGPL-3.0-or-later, dual-licensed under a separate commercial agreement.**
One codebase — you buy the *terms*, not a different artifact. See
`COMMERCIAL-LICENSE.md` for the full terms.

- Free/OSS use: AGPL-3.0 (see `LICENSE`). If you embed `nws` in software you
  distribute or expose over a network, your combined work is subject to AGPL.
- Commercial use without AGPL obligations:
  - Developer: $99/yr per named developer
  - Team: $499/yr up to 5 developers
  - Enterprise: $2,500–$10,000/yr per legal entity, priority support

  Self-hosting nws behind these terms is cheaper than metered API rental,
  keeps your data in your network, and is a one-line annual cost instead of
  per-request billing.

Derived from Apache-2.0 and MIT reference implementations — see `NOTICE` and
`THIRD_PARTY_LICENSES`.

## Roadmap

- [ ] `PyO3` / `napi-rs` bindings for the newspaper3k and JS audiences
- [ ] Hosted API tier on top of the same engine
- [ ] Benchmarks vs trafilatura-rs / readability / newspaper3k on a public corpus
