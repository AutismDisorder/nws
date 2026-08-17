# Changelog

All notable changes to nws are documented here, newest first.

## [0.1.0] — unreleased

Initial release: the newspaper3k / Mercury Parser / mozilla/readability
lineage rebuilt as a from-scratch, memory-safe Rust engine.

### Engine

- Full extraction pipeline: parse → score → clean → JSON-LD metadata →
  title/authors/date/lead-image → clean text, HTML, and LLM-ready Markdown.
- Readability `_grabArticle` port with flag-degradation retries, sibling
  expansion, multi-column alternative-candidate climbing.
- newspaper3k fallback extractor (stopword scoring) when readability comes
  up short, plus DocumentCleaner (`div_to_para`, link hoisting, drop caps).
- Mercury per-domain custom extractors (medium.com, pastebin.com,
  github.com, wikipedia, youtube, vimeo, tumblr, etc.), generic author/date
  chains, and multi-page `collectAllPages` (relative next-page links
  resolved).
- `isProbablyReaderable` pre-filter port; language detection (whatlang);
  keywords + extractive summaries (ported stopword NLP).

### Networking (`http` feature)

- tokio + reqwest (rustls) with a shared pooled client, 7s timeouts,
  browser user agents, gzip/brotli/deflate, retries.
- 10 MiB page cap enforced *while streaming* (gzip bombs and chunked
  responses abort the moment they cross it).
- Charset-aware decoding: `Content-Type` charset, `<meta charset>` sniff,
  lossy-UTF-8 fallback (GBK/Shift_JIS pages decode correctly).
- Image dimension checks read only the first 64 KiB chunk (headers, not
  pixels) with bounded concurrency.

### API server (`server` feature)

- `/health`, `/extract` (URL or HTML; `multipage=1`; content negotiation:
  JSON / Markdown / cleaned HTML), `/readerable`, `/batch`.
- Batch fetches concurrently (tokio, 16-way bounded) and extracts across
  all cores (rayon) inside the blocking pool; result order preserved.
- Batch size and aggregate byte caps; 502/422 status mapping;
  `NWS_FETCH_TIMEOUT` and `NWS_PORT` configuration.

### Performance

- ~517 µs per page (full pipeline including NLP and Markdown);
  ~14.7 ms for a 64-document rayon batch on 4 cores (~4,350 pages/s).

### Correctness and hardening

- 100% title / language / readerable parity on readability's 130-fixture
  corpus; 98.7% byline, 98.4% excerpt; ≥50% content recall on 129/130.
- 20,000 fuzz mutations with zero panics; clippy-clean on all targets and
  feature combinations.
- Fixed during audit: hostile relative-date overflow (remote-crash class),
  Medium embed percent-decoding, DIV→P score double-counting, `<br>`-chain
  collapsing across whitespace, all-caps byline mangling, legacy
  `DC.creator` meta key support, streaming size caps, and more — see
  `git log` for the full list.

### Licensing

- AGPL-3.0-or-later with a separate commercial license
  (see `COMMERCIAL-LICENSE.md`). Attribution to the upstream reference
  projects in `NOTICE` and `THIRD_PARTY_LICENSES`.
