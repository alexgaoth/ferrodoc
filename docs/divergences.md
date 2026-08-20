# Every failing document, and what it fails on

> **Understated, and measured so on 2026-08-19.** This census counts the
> documents the gates score. For the HTML reader that is eight
> `corpus/*.html` files, and it named six divergences.
> `scripts/sweep-epub-xhtml.sh` compares the **128** XHTML files inside the
> corpus EPUBs — the vocabulary pandoc's own writer emits — and finds **77**
> diverging, including two families named nowhere below: an empty
> `<section epub:type="titlepage">` and a link with no text. Five of the six
> are now fixed. Read the sweep before quoting a count from this page.

A census, not a change: nothing here alters behaviour. It exists because
two claims in the project's roadmap were carrying weight without a breakdown behind
them — that the HTML reader's 26 divergences are what holds
`corpus/epub-spec` at 8/22, and that closing them "costs three ways".
One of those is refuted below.

**Scope.** Every gate in `scripts/verify.sh` scoring under 100%, except
the two *fidelity* runs (`diff-latex` 0/11, `diff-rst` 2/11), which are
excluded on purpose: pandoc itself scores 0/11 and 3/11 on the same
corpus, so their failures measure the format, not this project, and 20
more rows of "the format cannot hold it" would drown the signal.

That leaves **51 documents** across nine gates:

| gate | score | failing documents |
|---|---|---|
| HTML reader (`diff-html-read`) | 633/659 | 26 |
| EPUB reader, spec chunks (`diff-epub`) | 8/22 | 14 |
| EPUB writer (`diff-epub-write`) | 8/11 | 3 |
| EPUB reader (`diff-epub`) | 10/12 | 2 |
| ODT reader (`diff-odt`) | 32/34 | 2 |
| GFM reader, spec (`diff-gfm`) | 651/652 | 1 |
| DOCX reader (`diff-docx`) | 36/37 | 1 |
| DOCX reader, LibreOffice (`diff-docx`) | 7/8 | 1 |
| DOCX writer (`diff-write`) | 10/11 | 1 |

Every path below is the harness's own `MISMATCH … at <path>` line. Repro
for the whole set:

```sh
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
cargo build --release -p ferrodoc-harness
H=./target/release/ferrodoc-harness
SPEC=corpus/commonmark-spec-0.31.2.json
$H diff-html-read $SPEC corpus > html.txt;   $H diff-gfm $SPEC       > gfm.txt
$H diff-docx corpus/docx > docx.txt;         $H diff-docx corpus/docx-libreoffice > lo.txt
$H diff-write corpus > write.txt;            $H diff-odt corpus/odt  > odt.txt
$H diff-epub corpus/epub > epub.txt;         $H diff-epub corpus/epub-spec > spec.txt
$H diff-epub-write corpus > epubwrite.txt
```

---

## The answer: which single fix buys the most documents

| # | group | docs | status |
|---|---|---:|---|
| **G1** | An element still open when its container closes (`<a>` never closed, or written `<a/>`) is reconstructed here and dropped by pandoc | **16** | 11 **declared deliberate** + 2 by extension, 3 actionable |
| **G2** | Pandoc's **EPUB** reader runs its HTML reader with `raw_html` **on**; ferrodoc's does not, so raw HTML pandoc keeps verbatim is normalised away | **11** | actionable, undeclared |
| **G3** | Trailing whitespace *inside* `<em>`/`<strong>` — pandoc hoists it out as `Space`, ferrodoc drops it | **3** | actionable, undeclared |
| **G4** | A start tag with no closing `>` — pandoc's tagsoup still builds a `Div` from the junk, ferrodoc emits nothing | **3** | actionable, declared as a family |
| **G5** | EPUB writer will not emit a reference the book cannot satisfy | **3** | **declared deliberate** |
| **G6** | `<![CDATA[…]]>` boundaries | **2** | actionable, declared as a family |
| **G7** | Pandoc reads every ODT list twice, so its identifier suffix is one higher | **2** | **declared deliberate** |
| — | eleven groups of one (listed in full below) | **11** | 2 deliberate, 9 actionable |

**Largest group: G1, 16 of 51 documents (31%).** But 13 of those 16 rest
on a *declared deliberate* decision — `COMPATIBILITY.md`, "HTML reader":
an `<a href="…"></a>` with no text is kept, because dropping it would
match pandoc on unclosed `<a>` tags at the price of deleting the
well-formed empty anchors real pages use as jump targets. Reversing that
is a policy change, not a bug fix.

Two of the 13 stretch that declaration rather than sit inside it: the
declared rule names `<a>`, and example 494 is on `<b>` while example 613
is on `<bab>`/`<c2c>`. The mechanism is identical and the count is
defensible, but the *decision* on record covers `<a>` only — the
surrounding html5ever-versus-tagsoup prose in `COMPATIBILITY.md` is
descriptive, not a choice anyone recorded making. Read the count as **11
declared, 2 by extension**.

**Largest actionable group: G2, 11 documents** — ten of the fourteen
`epub-spec` chunks plus `corpus/epub/corpus-truncation-cases.epub`.

**Largest group where the fix is provably *sufficient*: G1's duplicate
half and G2, three documents each.** A group count is a count of *first*
divergences; removing the first can expose a second. Only these six
documents are proven to have nothing behind the first cause:

- `<a/>` duplicate emission — spec examples 614, 615, 616. Ferrodoc's
  output is pandoc's plus one spurious trailing block; delete that block
  and the documents are identical.
- `raw_html` — `epub-spec/spec-10.epub`, `epub-spec/spec-17.epub`,
  `corpus/epub/corpus-truncation-cases.epub`. Their content XHTML already
  reads to **the same `blocks`** as `pandoc -f html`; the entire failure
  is the extension difference. (`blocks`, not the whole document: the
  `meta` objects differ in every HTML comparison here, for a reason of
  its own — see the closing section.)

For the other eight `epub-spec` chunks, G2 is a *prerequisite* rather
than a fix: a second divergence is measurably behind it (table below).

---

## Is `epub-spec` 8/22 really the HTML reader? No — not as stated

`corpus/epub-spec/generate.sh` and `COMPATIBILITY.md` both say each chunk
fails because it contains "one of the HTML reader's 26 known
divergences". Measured, that is wrong in three separate ways.

**1. The two gates score against different oracles.** `diff-html-read`
compares against `pandoc -f html`. `diff-epub` compares against
`pandoc -f epub` — whose embedded HTML reader has the `raw_html`
extension **enabled**, which `-f html` does not:

```sh
printf '<p>a <a href="/bar">b</p>\n' > /tmp/a.html
pandoc -f html          -t json /tmp/a.html   # Plain[a], Plain[b] — the <a> is dropped
pandoc -f html+raw_html -t json /tmp/a.html   # RawBlock "<p>", Plain[a], RawBlock "<a href=\"/bar\">", …
pandoc -f epub          -t json corpus/epub-spec/spec-00.epub  # same RawInline/RawBlock shape
```

Closing all 26 therefore cannot fix `epub-spec`, because the two targets
are contradictory: matching `pandoc -f html` on example 21 means emitting
**nothing** for `<a href="/bar\/)">`, while matching `pandoc -f epub`
means emitting `RawBlock html "<a href=\"/bar\\/)\">"`. No single reader
satisfies both without a raw-HTML mode. Ten of the fourteen failing
chunks diverge *first* on exactly this — a pandoc `Raw*` node ferrodoc
has no counterpart for.

**2. Two chunks contain no HTML reader divergence at all.** Unzip
`spec-10.epub` and `spec-17.epub` and read their `EPUB/text/ch001.xhtml`
with both readers: the `blocks` are identical. Their whole failure is G2.

```sh
unzip -qo corpus/epub-spec/spec-10.epub -d /tmp/s10
diff <(pandoc -f html -t json /tmp/s10/EPUB/text/ch001.xhtml | jq -S .blocks) \
     <(./target/release/ferrodoc -f html -t json /tmp/s10/EPUB/text/ch001.xhtml | jq -S .blocks) \
  && echo "blocks identical"
```

Compare `.blocks`, not the whole document: `meta` differs on every HTML
file in this corpus, independently of anything above, and comparing the
whole document hides the result being demonstrated behind a divergence
that has nothing to do with it. That third divergence is stated in the
closing section.

**3. Four chunks fail on HTML reader divergences that are not among the
26 and cannot be** — `diff-html-read` reads the CommonMark spec's
*expected* HTML, which never contains the constructs involved:

- `spec-13`, `spec-14`, `spec-15`: `<p><em>foo </em>bar**</p>` — pandoc
  hoists the trailing space out of the emphasis, ferrodoc drops it (G3).
- `spec-11`: `<code></code>` — pandoc drops an empty code span and the
  space beside it, ferrodoc emits `Code ""`.

The same is true of the EPUB gate proper: `COMPATIBILITY.md` says its two
misses "are in the 26 listed under the HTML reader". Neither is.
`corpus-truncation-cases.epub` is G2 (no HTML-level divergence exists),
and `corpus-code-and-raw.epub` is a space before `<br />` that pandoc
trims and ferrodoc keeps — a 27th HTML reader divergence, invisible to
`diff-html-read` for the same reason.

**What survives of the premise.** Seven of the fourteen chunks (`spec-00`,
`01`, `04`, `06`, `16`, `20`, `21`) do contain a G1 construct, so the
*family* named is real and the HTML reader is the main contributor. The
falsified part is the arithmetic: closing the 26 as `diff-html-read`
defines them would move `epub-spec` by **zero** documents on its own,
because every one of those seven also needs G2 first.

**And "costs three ways"?** One of the three holds cleanly, one holds
half, one does not.

- `diff-epub`'s two misses: **one** is an HTML reader divergence — the
  space before `<br />` in `corpus-code-and-raw.epub`, though not one of
  the 26. The other, `corpus-truncation-cases.epub`, is **not**: its
  XHTML produces the same `blocks` under both readers, so nothing the
  HTML reader does is wrong there. G2 is a missing extension in the EPUB
  path, not a divergence of the HTML reader as `diff-html-read` scores
  it, and the same distinction applies wherever G2 appears above.
- `corpus/epub-spec`: refuted, as measured throughout this section.
- The EPUB writer's three misses are **not** the HTML reader at all: all
  three are the declared rule that this writer does not emit a reference
  the book cannot satisfy (G5).

So **the 26** cost one gate and one document, not three gates. The HTML
reader at large costs more — `spec-11` and `spec-13`/`14`/`15` are HTML
reader divergences too — but they are not among the 26, which is what
the roadmap claim was about.

---

## Every failing document

### HTML reader — `diff-html-read $SPEC corpus`, 26 of 659

| document | first diverging path | cause |
|---|---|---|
| example 21 (Backslash escapes) | `/blocks` (1 vs 0) | G1 unclosed `<a>` kept as `Link`, pandoc drops it — *deliberate* |
| example 31 (Entity refs) | `/blocks` (1 vs 0) | G1 — *deliberate* |
| example 150 (HTML blocks) | `/blocks/0/c/1/0/c` (3 vs 1) | G1 (`<foo><a>` at end of a `<div>`) — *deliberate* |
| example 156 (HTML blocks) | `/blocks` (0 vs 1) | G4 `<div id="foo"` with no `>`; pandoc builds a `Div`, ferrodoc emits nothing |
| example 157 (HTML blocks) | `/blocks` (0 vs 1) | G4 |
| example 158 (HTML blocks) | `/blocks` (0 vs 1) | G4 |
| example 173 (HTML blocks) | `/blocks` (0 vs 1) | unclosed `<style>` swallows the rest of the document; pandoc recovers and reads `foo` |
| example 174 (HTML blocks) | `/blocks/0/c/0/c/1` (1 vs 2) | `</blockquote>` closing over an open `<div>`; pandoc keeps the following `<p>` inside the div, ferrodoc closes both |
| example 175 (HTML blocks) | `/blocks/0/c/0/0` | `<div>` inside `<li>` closed by `</li>`; pandoc loses the whole `<ul>`, ferrodoc keeps a `BulletList` |
| example 180 (HTML blocks) | `/blocks/0/c/0/c` | `<?php … ?>` processing instruction; pandoc drops it entirely, ferrodoc leaks `';` and `?>` as text |
| example 182 (HTML blocks) | `/blocks/0/c/0/c` | G6 `<![CDATA[…]]>` block; pandoc emits its contents as text, ferrodoc drops it |
| example 187 (HTML blocks) | `/blocks/0/c` (3 vs 1) | G1 — *deliberate* |
| example 191 (HTML blocks) | `/blocks` (2 vs 1) | `<pre>` inside `<tr>`; pandoc drops the table, ferrodoc emits `CodeBlock` + an empty `Table` |
| example 344 (Code spans) | `/blocks/0/c/0/c` | G1 (`<a href="`">`) — *deliberate* |
| example 476 (Emphasis) | `/blocks/0/c` (2 vs 1) | G1 — *deliberate* |
| example 477 (Emphasis) | `/blocks/0/c` (2 vs 1) | G1 — *deliberate* |
| example 494 (Links) | `/blocks/0/c` (6 vs 5) | G1, on `<b>` rather than `<a>` — *deliberate by extension*: the declared rule names `<a>` |
| example 613 (Raw HTML) | `/blocks` (2 vs 0) | G1, on `<bab>`/`<c2c>` — *deliberate by extension*: the declared rule names `<a>` |
| example 614 (Raw HTML) | `/blocks` (2 vs 1) | G1 duplicate: `<a/>` stays open here, so the block is emitted twice |
| example 615 (Raw HTML) | `/blocks` (2 vs 1) | G1 duplicate |
| example 616 (Raw HTML) | `/blocks` (2 vs 1) | G1 duplicate |
| example 629 (Raw HTML) | `/blocks/0/c/2/c` | G6 inline CDATA: ours `&<]]>`, pandoc `>&<` |
| example 630 (Raw HTML) | `/blocks/0/c` (3 vs 1) | G1 — *deliberate* |
| example 631 (Raw HTML) | `/blocks/0/c` (3 vs 1) | G1 — *deliberate* |
| example 642 (Hard line breaks) | `/blocks` (2 vs 0) | G1 — *deliberate* |
| example 643 (Hard line breaks) | `/blocks` (2 vs 0) | G1 — *deliberate* |

Probes behind G1, G4 and G6 (each run against pandoc 3.8.2.1 directly,
no whitespace normalisation anywhere):

```sh
printf '<p>a <a href="x">b</p>\n' | pandoc -f html -t json   # 2 Plains, no Link
printf '<p><a/></p>\n'            | pandoc -f html -t json   # ONE Para[Span]; ferrodoc emits two
printf '<div id="foo"\n*hi*\n'    | pandoc -f html -t json   # Div with *hi* as an attribute
printf '<p>foo <![CDATA[>&<]]></p>\n' | pandoc -f html -t json   # Str ">&<"
```

### EPUB reader, spec chunks — `diff-epub corpus/epub-spec`, 8 of 22

Two causes per row, because these compound: the *epub-level* first
divergence the gate reports, and what the same chunk's XHTML does under
`pandoc -f html` (blank = identical there, so the failure is G2 alone).

| document | first diverging path | epub-level cause | residual HTML-level cause |
|---|---|---|---|
| `spec-00.epub` | `/blocks/3/c/1/11/c/0` | G2 (pandoc `RawBlock`) | G1 unclosed `<a href="/bar\/)">` |
| `spec-01.epub` | `/blocks/1/c/1/1/c/0` | G2 | G1 unclosed `<a href="öö.html">` |
| `spec-04.epub` | `/blocks/1/c/1/28/c/1/0/c/0` | G2 | G1 duplicate trailing `Plain[Span]` |
| `spec-05.epub` | `/blocks/1/c/1/1/c/0` | G2 | attribute *order* in a malformed start tag: pandoc sorts the deduplicated `<div` last, ferrodoc keeps source order (G4 family) |
| `spec-06.epub` | `/blocks/1/c/1/6/c/0` | G2 (`<!-- foo -->`) | G1 unclosed `<a href="bar">` |
| `spec-10.epub` | `/blocks/1/c/1/8/c/0` | G2 (`<!-- -->`) | **none — G2 is the whole failure** |
| `spec-11.epub` | `/blocks/1/c/1/4/c` (3 vs 1) | empty `<code></code>`: pandoc drops it and the space beside it, ferrodoc emits `Code ""` | same |
| `spec-13.epub` | `/blocks/1/c/1/19/c/1/c` | G3 `<em>foo </em>bar**` | same |
| `spec-14.epub` | `/blocks/1/c/1/7/c/1/c` | G3 `<strong>foo </strong>bar baz**` | same |
| `spec-15.epub` | `/blocks/1/c/1/21/c/1/c` | G3 | same |
| `spec-16.epub` | `/blocks/1/c/1/14/c/0` | G2 | G1 on `<b>` (spec example 494) |
| `spec-17.epub` | `/blocks/1/c/1/14/c` (1 vs 3) | G2 (`<bar attr="](baz)">`) | **none — G2 is the whole failure** |
| `spec-20.epub` | `/blocks/1/c/1/1/c/0/c` | G2 (`<5001 foo>`) | G1 duplicate trailing `Plain[Span]` |
| `spec-21.epub` | `/blocks/1/c/1/1/c/0` | G2 | G1 unclosed `<a href="\*">` |

Repro for the residual column. It walks **every** content document in the
chunk: `spec-00.epub` is the one chunk pandoc split into two, and its
residual is in `ch002.xhtml` while `ch001.xhtml` matches — checking
`ch001` alone would report the row as wrong.

```sh
chunk=13   # or 00, where the residual is in the second content document
rm -rf /tmp/s$chunk && unzip -qo corpus/epub-spec/spec-$chunk.epub -d /tmp/s$chunk
for f in /tmp/s$chunk/EPUB/text/ch*.xhtml; do
    printf '%s: ' "$(basename "$f")"
    diff -q <(pandoc -f html -t json "$f" | jq -S .blocks) \
            <(./target/release/ferrodoc -f html -t json "$f" | jq -S .blocks) >/dev/null \
        && echo "blocks identical" || echo "blocks differ — the residual is here"
done
printf '<p><em>foo </em>bar</p>\n' | pandoc -f html -t json   # Emph, Space, Str — the Space is the divergence
```

`nav.xhtml` and `title_page.xhtml` diverge in **every** chunk, including
the eight that pass, so neither is a cause of any row above: pandoc's
EPUB reader treats them as furniture and never routes them through the
comparison. They are excluded from the table for that reason — but
excluded from *cause attribution* is not the same as containing nothing
worth counting, and reading the first as the second is how the two extra
divergences in the closing section went unnoticed for a round. What they
diverge on is audited there.

### EPUB reader — `diff-epub corpus/epub`, 10 of 12

| document | first diverging path | cause |
|---|---|---|
| `corpus/epub/corpus-code-and-raw.epub` | `/blocks/1/c/1/8/c/19/t` | a space before `<br />` — pandoc trims it, ferrodoc keeps `Space, LineBreak`. Not one of the 26. Probe: `printf '<p>a <br /> b</p>\n' \| pandoc -f html -t json` |
| `corpus/epub/corpus-truncation-cases.epub` | `/blocks/1/c/1/9/c/0/c` | G2 — an HTML comment pandoc keeps as `RawInline`; its XHTML produces the same `blocks` under both readers, so this is not an HTML reader divergence |

### EPUB writer — `diff-epub-write corpus`, 8 of 11

All three are one **declared deliberate** rule (G5): this writer does not
emit a reference the book cannot satisfy, and `epubcheck` rejects
pandoc's book for exactly the references pandoc does emit (`RSC-007`).
See `COMPATIBILITY.md`, "EPUB writer".

| document | first diverging path | cause |
|---|---|---|
| `corpus/images.md` | `/blocks/1/c/1/5/c/0/c` | image whose bytes are missing becomes its alt text — *deliberate* |
| `corpus/readme-style.md` | `/blocks/1/c/1/4/c/1/5/c/10/c` | same — *deliberate* |
| `corpus/nested-structures.md` | `/blocks/1/c/1/3/c/2/c/10/c` | relative link naming no file in the book becomes its text — *deliberate* |

### ODT reader — `diff-odt corpus/odt`, 32 of 34

Both are one **declared deliberate** rule (G7): pandoc reads every list
twice (2^n times at n levels), so its identifier suffix is one higher.
Copying an exponential blowup into a converter whose promise is that it
cannot be made to hang is the worse trade.

| document | first diverging path | cause |
|---|---|---|
| `corpus/odt/spec-03.odt` | `/blocks/4/c/0/0/c/1/0` | `foo-1` here, `foo-2` in pandoc — *deliberate* |
| `corpus/odt/spec-09.odt` | `/blocks/27/c/0/0/c/1/0` | `foo` here, `foo-1` in pandoc — *deliberate* |

### The five singletons in other gates

| gate | document | first diverging path | cause |
|---|---|---|---|
| GFM reader (spec) | example 98 (Setext headings) | `/blocks` (2 vs 0) | `---\n---\n` is an empty YAML metadata block to pandoc's `gfm` and two `HorizontalRule`s here; YAML metadata is a pandoc extension the GFM specification does not define — *deliberate*, `COMPATIBILITY.md` "GFM" |
| DOCX reader | `corpus/docx/spec-09.docx` | `/blocks/32/c` (1 vs 2) | a list nested inside a table cell is flattened; the `BulletList` is lost |
| DOCX reader (LO) | `corpus/docx-libreoffice/minutes.docx` | `/blocks/7/c` (one side only) | LibreOffice writes a horizontal rule as a paragraph with nothing but a bottom border; ferrodoc reads `HorizontalRule`, pandoc reads nothing — *deliberate* |
| DOCX writer | `corpus/nested-structures.md` | `/blocks/1/c/0` (1 vs 3) | a quotation nested in a way the DOCX round trip does not preserve |

---

## What this suggests, without doing any of it

Stated as findings, not as a plan — ranking belongs in the roadmap.

- **`epub-spec` cannot be fixed by the HTML reader.** It needs a
  raw-HTML mode in the EPUB path first (G2, 11 documents). Ten of the
  fourteen chunks do not even reach their HTML-level divergence today.
- **Three of the four documented claims about these gates are wrong**
  and should be corrected whatever else happens:
  `corpus/epub-spec/generate.sh` and `COMPATIBILITY.md` "EPUB reader"
  both attribute the chunk failures to the 26; `COMPATIBILITY.md` says
  the EPUB gate's two misses "are in the 26" and neither is.
- **The 26 are 10 causes, not one**, and 13 of the 26 documents are a
  decision already taken rather than work outstanding — 11 of them
  declared, 2 covered only by extension of it. The actionable
  HTML reader count is **13**, in nine groups, the largest of which
  (`<a/>` duplicate emission) is three documents.
- **Five HTML reader divergences exist that no gate measures**, all five
  invisible to `diff-html-read` because neither the CommonMark spec's
  expected HTML nor the eight `corpus/*.html` files contains the
  construct. The last two are in the EPUB furniture files excluded from
  the chunk table above, which diverge in the **passing** chunks
  (`spec-02`, `spec-03`) as well as the failing ones — so no gate can
  reach them however the scores move:
  - trailing space inside `<em>`/`<strong>` (G3), pandoc hoists it out;
  - a space before `<br />`, pandoc trims it and ferrodoc keeps it;
  - **`<head>` metadata**: pandoc's HTML reader populates `meta` from
    `<title>` and the `<meta>` elements, and ferrodoc emits `{}`. The
    spec's expected HTML is fragments with no `<head>`, so no case in
    the corpus reaches it — although
    `crates/ferrodoc-harness/src/main.rs:434` does compare the whole
    JSON document, `meta` included, so a corpus with one full page in it
    would score this today. Probe:

    ```sh
    printf '<html><head><title>T</title></head><body><p>x</p></body></html>' > /tmp/h.html
    pandoc -f html -t json /tmp/h.html   # "meta":{"title":{"t":"MetaInlines",…}}
    ./target/release/ferrodoc -f html -t json /tmp/h.html   # "meta":{}
    ```

  - **An `id` on `<li>` is dropped** — pandoc wraps the item's content in
    a `Span` carrying it, ferrodoc emits the content bare. This is silent
    *identifier* loss, not a formatting difference: it is what every
    `nav.xhtml` in `corpus/epub-spec` diverges on. Probe:

    ```sh
    printf '<ol><li id="toc-li-1"><a href="a.html">T</a></li></ol>\n' > /tmp/li.html
    pandoc -f html -t json /tmp/li.html   # Plain[ Span ["toc-li-1",[],[]] [ Link … ] ]
    ./target/release/ferrodoc -f html -t json /tmp/li.html   # Plain[ Link … ] — no Span
    ```

  - **`epub:type` is not read as a class, and `epub:type="titlepage"`
    is kept** — two facets of one attribute, and what every
    `title_page.xhtml` diverges on. Pandoc turns the attribute's value
    into a **class** and keeps the attribute too; ferrodoc keeps only
    the attribute. Separately, on the value `titlepage` pandoc drops the
    element *and everything inside it*, where ferrodoc emits a `Div`.
    Neither the element nor emptiness is the trigger — a `<section>`
    without `epub:type` matches pandoc exactly, which is why
    `corpus/sectioning.html` passes. Probe both facets:

    ```sh
    printf '<section epub:type="titlepage"><p>x</p></section>\n' > /tmp/tp.html
    pandoc -f html -t json /tmp/tp.html   # "blocks":[]
    ./target/release/ferrodoc -f html -t json /tmp/tp.html   # one Div, content intact

    printf '<section epub:type="chapter"><p>x</p></section>\n' > /tmp/ch.html
    pandoc -f html -t json /tmp/ch.html | jq -c '.blocks[0].c[0]'    # ["",["section","chapter"],[["epub:type","chapter"]]]
    ./target/release/ferrodoc -f html -t json /tmp/ch.html | jq -c '.blocks[0].c[0]'   # ["",["section"],[["epub:type","chapter"]]]

    printf '<section class="x"><p>y</p></section>\n' > /tmp/plain.html   # control: no epub:type
    diff <(pandoc -f html -t json /tmp/plain.html | jq -S .) \
         <(./target/release/ferrodoc -f html -t json /tmp/plain.html | jq -S .) && echo "identical"
    ```

  Four of the five were found only because the EPUB corpus contains HTML
  the spec's expected output never writes — a new format finding bugs in
  the old code, for the third time. The `<head>` one was found by this
  document getting it wrong, and the last two by its getting the count of
  those wrong; both are the next entry.

- **This document has twice been an instance of the defect it
  catalogues**, and both entries are left here rather than quietly
  fixed. (No ordinal: the roadmap counts these too, and the two tallies
  cannot be reconciled without deciding cases the tallies never named.
  The rule below is the part that matters and does not need a number.)
  Documented claims in this repo keep turning out wider than the code
  behind them; `COMPATIBILITY.md`'s "the two documents it misses
  are both HTML reader divergences … in the 26" is one of them, found
  above. The first revision of *this* file then claimed, in bold, that
  three documents read "byte-identically" to `pandoc -f html`, and
  printed a `diff` as the proof. Run as printed, that command fails —
  because of the `<head>` metadata divergence, which nobody had noticed
  precisely because no gate measures it. The `blocks` claim was true and
  the conclusion held; the word and the repro were wider than the
  evidence. **A repro that has not been run as printed is a claim, not
  evidence** — which is the rule the rest of this census is built on,
  applied to the census.

  The second revision then said "three divergences no gate measures"
  when there are five. The two it missed were in the furniture files it
  had itself dismissed one section earlier, with the words "neither is a
  cause of anything" — a sentence that is true about *cause attribution*
  and was silently read as "neither contains anything worth counting".
  One of the two is silent identifier loss. **Excluding something from a
  count needs its own evidence, not the evidence that excluded it from a
  different count** — the same defect as the first, one level up: not an
  unrun command this time, but an unasked question.
