# ferrodoc

A universal document converter in Rust, pandoc-compatible at the AST level.

Phase 1 (in progress): pandoc-JSON-interoperable AST (`ferrodoc-ast`), a
CommonMark reader differentially tested to produce ASTs identical to
`pandoc -f commonmark` (`ferrodoc-markdown`), HTML and plain-text writers,
and the differential harness (`ferrodoc-harness`).

## Differential testing

```sh
cargo run -p ferrodoc-harness -- diff-spec corpus/commonmark-spec-0.31.2.json
cargo run -p ferrodoc-harness -- diff-ast corpus
```

Both compare our AST against `pandoc -f commonmark -t json` (requires pandoc
3.8.x on PATH). Current conformance: 652/652 CommonMark spec examples (100%).

`corpus/commonmark-spec-0.31.2.json` is the example set from the
[CommonMark spec](https://spec.commonmark.org/0.31.2/), © John MacFarlane,
licensed CC-BY-SA 4.0.
