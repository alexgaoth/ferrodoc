# ferrodoc

A universal document converter in Rust, pandoc-compatible at the AST level.

Phase 1 status:

- **`ferrodoc-ast`** — the complete pandoc-types 1.23 AST; any `pandoc -t json`
  document round-trips losslessly (value-equal JSON).
- **`ferrodoc-markdown`** — CommonMark reader producing ASTs identical to
  `pandoc -f commonmark -t json`: **652/652 CommonMark 0.31.2 spec examples
  (100%)** plus 1,000+ adversarial/fuzz documents. Two known residual
  divergence families (entity-encoded spaces in `Str` tokens; some
  refdef+dash-run corner cases) are documented in `.iterate/` audit trails.
- **`ferrodoc-html`** — HTML writer identical to
  `pandoc -f commonmark -t html --syntax-highlighting=none --wrap=none`
  wherever the AST matches: 652/652 spec examples, zero writer-attributable
  divergences across all fuzz corpora.
- **`ferrodoc-text`** — best-effort plain-text extraction (for indexing/LLM
  ingestion); deliberately not `pandoc -t plain` byte-parity.
- **`ferrodoc-harness`** — the differential oracle: `diff-ast`, `diff-spec`,
  `diff-html` compare against a live pandoc; `bench` measures throughput.

## Differential testing

```sh
cargo run -p ferrodoc-harness -- diff-spec corpus/commonmark-spec-0.31.2.json --fail-under 100
cargo run -p ferrodoc-harness -- diff-ast corpus --fail-under 100
cargo run -p ferrodoc-harness -- diff-html corpus/commonmark-spec-0.31.2.json --fail-under 100
```

All compare against pandoc 3.8.x on PATH.

## Benchmarks (informal)

`cargo build --release && target/release/ferrodoc-harness bench <file>`,
Linux x86-64, pandoc 3.8.2.1. This compares the in-process library call
(parse + write HTML) against invoking the `pandoc` subprocess per document —
i.e. what a pipeline actually pays in each model:

| input | ferrodoc (in-process) | pandoc (subprocess) | ratio |
|---|---|---|---|
| 15.5 KB (concatenated spec examples) | 1.97 ms | 137.4 ms | ~70× |
| 0.6 KB (`corpus/readme-style.md`) | 45 µs | 25.1 ms | ~554× |

The small-document case is dominated by pandoc's process startup — which is
exactly the cost every per-document subprocess pipeline pays today.

## License notes

`corpus/commonmark-spec-0.31.2.json` is the example set from the
[CommonMark spec](https://spec.commonmark.org/0.31.2/), © John MacFarlane,
licensed CC-BY-SA 4.0.
