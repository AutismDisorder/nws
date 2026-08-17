# Benchmarks

Measured with `cargo bench` (criterion, release profile, LTO-thin,
codegen-units=1) on a 4-core machine, synthetic 5-paragraph page.

## Throughput

| workload            | time     | throughput     |
|---------------------|----------|----------------|
| single page, full pipeline | ~517 µs | ~1,930 pages/s |
| batch of 8 (rayon)  | ~1.86 ms | ~4,300 pages/s |
| batch of 32 (rayon) | ~7.3 ms  | ~4,380 pages/s |
| batch of 64 (rayon) | ~14.7 ms | ~4,350 pages/s |
| Markdown render (prepared doc) | ~27 ns | — |

The single-page number includes the whole pipeline: HTML5 parse, scoring,
cleaning, JSON-LD metadata, title/author/date extraction, language
detection, NLP keywords + summary, text + Markdown rendering.

Network throughput is dominated by the upstream site; the engine's own
per-page cost is the table above, and the HTTP layer uses one shared
connection pool with per-chunk size caps.

## Quality (readability corpus, 130 fixtures)

| metric                        | result   |
|-------------------------------|----------|
| title parity                  | 130/130 (100.0%) |
| language parity               | 73/73 (100.0%) |
| readerable parity             | 130/130 (100.0%) |
| byline parity                 | 75/76 (98.7%) |
| excerpt parity                | 126/128 (98.4%) |
| content recall ≥ 50%          | 129/130 (99.2%) |

The remaining mismatches are cases where the reference itself is lossy
(dateline-as-byline fixtures, entity-decoding corner cases).

## Robustness

- 20,000 structured fuzz mutations (random tag soup, invalid UTF-8, deep
  nesting, hostile attributes): **zero panics**.
- Clippy-clean across all targets and feature combinations, including the
  no-default-features core build.
- Hostile-input classes fixed in audit: relative-date integer overflow,
  gzip bombs, oversize chunked responses, unbounded batch requests.
