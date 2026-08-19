# ferrodoc-ipynb

Jupyter notebooks. JSON around markdown, so it depends on
`ferrodoc-markdown` and on nothing else. Gates: `diff-ipynb` (reader) and
`diff-ipynb-write` (writer), both **8/8** on `corpus/ipynb-handmade`.

- **The oracle is `pandoc -f ipynb -t json`, not the nbformat schema.**
  The two disagree, and every rule in `read.rs` is commented with what the
  3.8.2.1 binary does. Four that are easy to guess wrong:
  - `execution_count` **leads** the cell attributes; the metadata keys are
    sorted after it. Sorting it in among them scores **0/8**.
  - a mime bundle contributes **one** block, chosen by
    image → `+json` → `text/plain` → `text/html` → `text/latex` →
    `text/markdown`. `text/plain` beating `text/html` is not the obvious
    order and it is the binary's — a DataFrame comes out as its repr.
  - an ANSI escape in a traceback is stripped **from `ESC` to the next
    `m`**, and to the *end of the text* when there is no `m` left, so
    `x\x1b[K y\n` becomes `x`. It is a quirk; the gate scores the quirk.
  - null is `""` in notebook metadata and the four characters `null` in a
    cell attribute. Same JSON, two spellings.
- **Cell markdown is neither CommonMark nor GFM.** Pandoc's ipynb
  extension set has pipe tables, task lists, strikeout, `$…$` math, raw
  HTML and **classed** bare-URI autolinks (`Link ("",["uri"],[])`, and
  `["email"]` with a `mailto:` target); it lacks footnotes, escaped line
  breaks, `www.` autolinks and `fancy_lists`. `read_gfm` is the closest and
  now reads the math; `autolink_class` adds the classes, using the only
  signal comrak leaves — an autolink's text *is* its target — so an explicit
  `[url](url)` is classed where pandoc leaves it bare. `fixup_markdown`
  flattens every `OrderedList` to `(1, DefaultStyle, DefaultDelim)`, the
  *only* ordered list that set can parse. Remaining divergences are in
  `COMPATIBILITY.md` with a command each. **Fix one before the corpus
  carries it — but never keep it out of the corpus to protect a score:**
  the first version of these gates read 100% partly because no notebook in
  it contained an equation.
- **The writer emits GFM, not CommonMark**: `write_markdown` targets
  CommonMark, which has no table syntax, so a table becomes one paragraph
  per cell and the writer gate drops to 7/8. `write_gfm` keeps it.
- **An image is extracted, never inlined.** The AST names
  `<sha1 of the bytes>.<ext>`, which is how pandoc's media bag names it, so
  `sha1.rs` exists to match a filename and nothing else. Going the other
  way, a markdown-cell image becomes an `attachment:` keyed by the **whole
  URL**, not its basename — that is what pandoc does, and it is why a
  picture gains a cell-id prefix on every round trip.
- **`pandoc -f json -t ipynb` exits 99** when an `Image` names a file it
  cannot find, so the writer gate writes this crate's media bag to a
  directory and passes it as `--resource-path` to both writers.
- **Cell ids are derived, not random.** nbformat 4.5 requires one; pandoc
  draws a UUID at random, this derives a UUID-*shaped* string from the
  cell's content so a notebook is reproducible. The gate clears that shape
  from both sides and nothing else — a real Jupyter id is 8 hex characters
  (`3a7f1c2d`) and must still fail. Mutation-tested: a writer that discards
  identifiers scores **0/8**.
- **The judge that is not pandoc is `nbformat.validate`**
  (`scripts/nbformat-check.py`, in CI). Two writers agreeing with each
  other can be wrong together; the Jupyter server is what actually refuses
  a notebook.
