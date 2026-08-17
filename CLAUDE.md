# CLAUDE.md

Pandoc-compatible document converter, built to be adopted outside this repo.
Two contracts: output must be value-identical to pandoc's, proven
differentially and never assumed; and every advantage claimed to a reader
must be a number someone else can reproduce.

Per-crate gotchas live in `crates/*/CLAUDE.md` and
`bindings/python/CLAUDE.md`; this file is what holds repo-wide.

## Commands

- Toolchain lives off default PATH on this machine:
  `export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"` (cargo + pandoc 3.8.2.1).
  Conformance claims are pinned to pandoc 3.8.2.1; a different pandoc will
  produce spurious diffs.
- **`./scripts/verify.sh` decides whether the tree is releasable** — tests,
  clippy, wasm32, the memory bound and all 20 differential gates. The
  bindings live outside the workspace and need their own runs: `--wasm`
  (npm, including a headless browser), `--c` (the C ABI under valgrind),
  `--fuzz` (500k mutations, after any reader change). Every threshold is in
  that script and nowhere else, and CI calls it. Never report a gate from a
  piped command: `| tail` masks the exit status, which is how a failing
  publish once read as success.
- Never run `cargo fmt`: the repo is not fmt-clean (29 files differ), so it
  buries a surgical change in unrelated reformatting. Match surrounding style
  by hand.
- `crates/ferrodoc` is the entry point (facade + `ferrodoc` binary): a new
  reader or writer is unreachable by users until added to its `Format` enum,
  `parse`/`render`, and the `--help` text in `main.rs`.
- Releasing is `cargo publish --workspace`, never a loop over
  `cargo publish -p`: a loop keeps going after a failure, so one error
  becomes six. Every internal dependency needs a **`version` as well as a
  `path`** or publish refuses it, and `ferrodoc-harness` stays
  `publish = false` — it shells out to pandoc and reads the corpus.

## Rules

- **A roadmap item's premise is a claim like any other: measure it before
  building on it.** Two in `TODO.md` were false — "`HTML -> AST` is
  superlinear" (it is linear; the harness was timing a refusal) and "pandoc
  emits `RawBlock` for unknown elements" (it drops the tag, as this does).
  Each would have been days of work on the wrong thing.
- Never guess pandoc behavior — probe it first
  (`printf '...' | pandoc -f commonmark -t json`), then encode the probed rule
  with a comment. Every quirk in ferrodoc-markdown was derived this way.
- **Probe with `-t json`, and never normalize whitespace in the output you
  are comparing.** A `re.sub(r'\s+', ' ')` over pandoc's `-t native` output
  collapses the very thing being measured: it turned "three spaces survive"
  into "one space", and two ODT rules were derived backwards from that
  before the tooling was suspected rather than the binary.
- **Pandoc's GitHub sources describe a later pandoc than the 3.8.2.1 binary**
  and disagree with it. Read them for algorithm shape only; the binary decides.
- **A corpus of your own output proves less than it looks.** `corpus/docx`
  and `corpus/odt` are pandoc's output, so a gate over them cannot fail on
  a structure pandoc's writer never emits — which is most of what a word
  processor emits. The `-libreoffice` corpora beside them (`bash
  corpus/<name>/generate.sh`) are the only evidence either reader handles
  anything else, and each found a real bug the pandoc corpus could not.
- **`diff-html` scores against the CommonMark specification, which has no
  tables in it at all**, so nothing in the HTML writer's table handling is
  gated by it — and `diff-md`/`diff-gfm-md` are round trips through this
  project's own reader, so an inline the reader never produces (there is
  no `^x^` in CommonMark) never reaches the writer either. Three silent
  losses lived in those two blind spots with every gate green: column
  alignment, column widths, and `Superscript`/`Subscript`/`Underline`/
  `SmallCaps`/`Span` attributes. `samples/` is what found them — run
  `./samples/generate.sh` after any writer change and read the diffs.
- Generator inputs live in `<corpus>/src/`, and the HTML collector skips
  `src/`. `diff-html-read` walks `corpus/` for `*.html`, so without that
  rule eight DOCX sources silently widened the HTML gate — and *passed*, so
  the score rose and nothing looked wrong.
- **A corpus input that `.gitignore` hides makes local and CI measure
  different corpora.** `corpus/docx/src/logo.png` was ignored, so every
  media-resolving gate found the picture here and nothing on a runner:
  `diff-epub-write` read 72.7% locally and 63.6% in CI, and the gate that
  disagreed was the one that had just been written. Check
  `git ls-files --others --ignored --exclude-standard corpus/` is empty
  before trusting a number that involved media.
- A behavior with no corpus document that fails without it is not covered.
  Mutation-test a new rule by breaking it and confirming the corpus drops;
  restore with a `cp` of a copy taken first, never `git checkout`.
- A round-trip gate cannot see a rule whose two spellings read back the same
  (`- ☐ a` and `- [ ] a` are one AST). Those need a test on the literal
  output, or they ship broken with CI green.
- A percentage threshold over a large corpus is not a regression gate: 99%
  of 654 tolerates five failures. Gate a small hand-written corpus at 100
  separately from the spec run.
- Code no input has ever reached is not code that works, and every gate is
  green while it stays unreached. Making a dormant path live — `docx → docx`
  first embedding a picture, a fixture stopping being skipped — is a change
  to verify end to end, not by its diff. Both shipped breakage that way.
- **Before publishing any number, read `docs/benchmarking.md`** — and note
  that a timing loop discarding its `Result` measures refusal, not work.
- Measure every optimization against the code it replaces, and *ablate
  first* to find where the cost is: deleting the DOCX tree won 5× where
  interning its names would have won 8%. Revert what does not measure.
- A README claim needs its reproducing command, figures from one sitting, and
  pandoc's advantages beside it — selling wins without limits is what makes a
  reader stop trusting the wins.

## Gotchas

- Every *workspace* crate must keep compiling for `wasm32-unknown-unknown`,
  so no crate below the facade may do IO or pull in a C library.
  `bindings/python` is outside the workspace and exempt — see its own
  `CLAUDE.md`.
- `ferrodoc-docx` exposes `xml` and `media` as `#[doc(hidden)] pub`, and
  `ferrodoc-odt` and `ferrodoc-epub` are built on them: all three formats
  are zips of XML parts. Extend them there rather than copying, and keep
  them out of the rendered documentation.
- **A new format finds bugs in the old code.** EPUB found two in the HTML
  reader that `diff-html-read` could not see — a `<section id>` leaking
  its identifier onto the heading, and `<span class="smallcaps">` not
  becoming `SmallCaps`. Run the other gates after adding one; the win is
  usually not confined to the new crate.
- All readers bound their recursion and must return `Err`, never abort or
  truncate. Keep bounds low: a test thread gets 2 MiB, so 500 overflows the
  suite the bound exists to protect; 200 works.
- A depth bound protects nothing a walk can bypass — a helper that never
  calls it, or dropping the tree, which `Rc` does recursively (hence
  `flatten()`). Walk iteratively wherever the bound is not consulted.
- `Vec::remove`/`insert(0, …)` inside a scan is quadratic and has shipped four
  times here; ordinary pages hit it (200k `x<br>`+newline never finished).
  Build a new `Vec` in one pass, or use `retain`/`drain`. Same trap in
  identifier uniquing: resume the `-N` suffix search, never restart it.
- Known gaps live in each crate's docs; `.iterate/*/` holds the critic verdicts
  behind them, including fixes made after a run hit its round cap.
  `COMPATIBILITY.md` has every known loss with its reproducing command —
  update it when a number moves.
- **Some formats have no oracle.** Pandoc writes `AsciiDoc` and cannot
  read it, so that writer has no differential gate at all and is judged by
  `asciidoctor` in CI. Where a format's own toolchain exists, it is the
  better judge anyway: `pdflatex`, `sphinx-build -W`, `epubcheck`.
- `unsafe` is forbidden workspace-wide, allowed **for the attribute only**
  in `bindings/wasm` (a handle table is what avoids the blocks), and
  allowed with real blocks in `bindings/c` alone — where each is one
  dereference wide and a test fails the build if one spans logic.
- A **text** writer is gated on *fidelity* (write it, read it back, require
  the original) with pandoc's score printed beside it; a **binary** writer
  is gated against pandoc's own output through pandoc's reader. The
  difference is whether pandoc's writer is lossy enough that matching it
  would copy its losses — it is for markdown and LaTeX, and is not for
  DOCX and ODT.
- **`TODO.md` picks the next item; it is not a wish list.** It holds the bet,
  three ranking rules and a five-step procedure. Read it before starting
  anything unprompted, and when an item lands, *re-run the ranking and
  rewrite the order* — skipping that decays it into a list.
