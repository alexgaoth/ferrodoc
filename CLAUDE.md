# CLAUDE.md

Pandoc-compatible document converter. The contract everywhere: output must be
value-identical to pandoc's, proven differentially — never assumed.

## Commands

- Toolchain lives off default PATH on this machine:
  `export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"` (cargo + pandoc 3.8.2.1).
  Conformance claims are pinned to pandoc 3.8.2.1; a different pandoc will
  produce spurious diffs.
- Verify any reader/writer change with all three gates before claiming done:
  `cargo run -q -p ferrodoc-harness -- diff-spec corpus/commonmark-spec-0.31.2.json --fail-under 100`
  `cargo run -q -p ferrodoc-harness -- diff-ast corpus --fail-under 100`
  `cargo run -q -p ferrodoc-harness -- diff-html corpus/commonmark-spec-0.31.2.json --fail-under 100`
  plus `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`
  (pedantic + missing_docs are deny-level).
- AST fixtures are genuine pandoc output: after touching them, `bash
  crates/ferrodoc-ast/tests/fixtures/generate.sh && git diff` must be clean.

## Rules

- Never guess pandoc behavior — probe it first
  (`printf '...' | pandoc -f commonmark -t json`), then encode the probed rule
  with a comment. Every quirk in ferrodoc-markdown was derived this way.
- pandoc-types Haskell `Int` fields are **i64** in ferrodoc-ast, and the
  serde pattern is adjacently-tagged `t`/`c` enums + `from`/`into` positional
  repr structs — keep both when adding types (JSON shape must not change).

## Gotchas

- comrak sourcepos **end** lines are unreliable for unclosed code/HTML blocks
  (multi-line nodes report `[1..1]`); use `NodeCodeBlock::closed` and
  `start.line + literal line count` instead.
- comrak already merges adjacent Text nodes — do not add Str coalescing to the
  reader. The one normalization pandoc needs on top: merge directly-adjacent
  same-type Emph/Strong siblings.
- HTML escaping differs by context: text `&<>` only; attributes AND code
  blocks additionally `"`→`&quot;`, `'`→`&#39;`; inline code escapes like
  text (no quote escaping). Verified against pandoc; don't "unify" them.
- Two known reader divergence families are OPEN (don't rediscover):
  entity-encoded spaces (`&#32;`) should stay inside `Str`, and
  refdef+dash-run→HorizontalRule corner cases. Repros and analysis:
  `.iterate/20260810-markdown-reader/round-3-verdict.md`.
