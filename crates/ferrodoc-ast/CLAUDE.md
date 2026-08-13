# ferrodoc-ast

The document model, wire-compatible with pandoc's JSON. Gated by `diff-ast`
— see the root `CLAUDE.md`.

- pandoc-types Haskell `Int` fields are **i64** here, and the serde pattern
  is adjacently-tagged `t`/`c` enums plus `from`/`into` positional repr
  structs. Keep both when adding a type: the JSON shape must not change.
- The test fixtures are genuine pandoc output. After touching them, `bash
  tests/fixtures/generate.sh && git diff` must be clean.
