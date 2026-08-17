# Samples — what the conversions actually look like

The gates in `COMPATIBILITY.md` report percentages. This folder is the same
question answered by artefacts you can open: **for each important
conversion, ferrodoc's output and pandoc's output, side by side, with the
diff between them.**

Regenerate with `./samples/generate.sh` (needs pandoc 3.8.2.1). Start with
`RESULTS.md` for the summary table.

## How to read a folder

```
06-markdown-to-html/
    ferrodoc.html   what ferrodoc produced
    pandoc.html     what pandoc produced from the same input
    diff.txt        pandoc → ferrodoc, absent when they are identical
```

Inputs are in `inputs/`. Two of them are **not** written by this project,
on purpose: `report.docx` and `contract.odt` came out of LibreOffice, and
`page.html` was written by pandoc. A tool that is only ever tested on its
own output tells you nothing. `handbook.md` is written for this folder and
deliberately contains the awkward cases — nested quotes, a table with a
right-aligned column, raw HTML, entities, unicode, a footnote, an image
with a title, inline code containing a backtick.

For the three binary targets there is nothing to read, so what is compared
is **what the rest of the world sees when it opens the file**: pandoc
reading ours against pandoc reading its own (`*.readback.md`). The `.docx`,
`.odt` and `.epub` are kept too — open them in Word, LibreOffice or an
e-reader.

### One thing that is normalised, and why

ferrodoc has no line-wrapping option: it never reflows text. Pandoc reflows
at column 72 by default, which rewrites nearly every line of every text
sample and buries the differences that are about content. So pandoc is run
both at `--wrap=none` and `--wrap=preserve`, the closer output is kept, and
**the flag that won is printed in the results table** rather than chosen
quietly. Nothing else is normalised.

## The verdict, honestly

**Where it is exact.** DOCX → HTML, EPUB → markdown, and the DOCX and ODT
writers are byte-identical to pandoc or differ only in an internal media
filename. `report.docx` → markdown is content-identical; the 14-line diff
is entirely markdown *spacing* — pandoc writes `1.  item` and a padded
table, ferrodoc writes `1. item` and an unpadded one. Both re-read to the
same document. If your pipeline is "Word in, markdown out for an index",
which is the case this project was built for, the output is the same
document pandoc gives you.

**Where it differs and that is the point.** LaTeX, RST and AsciiDoc are
gated on *fidelity*, not on matching pandoc — pandoc's own LaTeX round trip
scores 0/11 on the same corpus, so copying its output would mean copying
its losses. Most of the 230-line LaTeX diff is pandoc's preamble, which is
large by design where ferrodoc's is minimal by design so the output
compiles on a base TeX. These three are judged by `pdflatex`,
`sphinx-build -W` and `asciidoctor` in CI instead, and all three pass.

**Where it is a real gap.** Three, and they are the reason to look:

| gap | what you see | status |
|---|---|---|
| **footnotes are not parsed** | `[^note]` survives as literal text in *every* output | deliberate scope, and the most visible limitation here |
| **task lists render as `☒`/`☐`** | pandoc emits a real `<input type="checkbox">` and `class="task-list"` | not deliberate; found by these samples, unfixed |
| **`plain` output is plainer** | quotes not indented, tables tab-separated rather than column-aligned | minor format, low priority |

The footnote one deserves a sentence of its own. Footnotes are a *pandoc*
extension rather than part of the GFM specification, so not reading them is
a defensible line — but it is drawn where a lot of real markdown sits, and
in the samples it is the difference you will notice first. If your
documents use footnotes, this is currently the wrong tool for them.

## What building this folder found

Three real defects, all of them **silent data loss**, all fixed and each
now covered by a test:

1. **The HTML writer dropped table column alignment.** `|---:|` produced no
   `text-align` at all, so every right- or centre-aligned column in every
   converted table came out left-aligned.
2. **The HTML writer dropped table column widths.** The DOCX reader
   recorded a word processor's widths exactly and the writer threw them
   away, so every converted table came out equal-width.
3. **The markdown writer dropped superscript, subscript, underline, small
   caps and span attributes.** This is worse than losing styling: `H~2~O`
   came out as `H2O`, `E=mc^2^` as `E=mc2`, and an anchor that links
   pointed at simply disappeared. Pandoc writes raw HTML for all of these
   and now so does ferrodoc, byte for byte.

None of the three could be caught by any existing gate, and for the same
reason in each case: `diff-html` scores against the **CommonMark
specification**, which contains no tables at all, and `diff-md` is a round
trip through ferrodoc's own reader, which never produces a `Superscript`
because CommonMark has no `^x^`. The gates were green and the output was
wrong. That is what this folder is for, and it is worth re-running it after
any writer change.
