# Field Handbook

A realistic document, written the way people actually write them — and
deliberately including the awkward parts, because a sample that only shows
easy cases tells you nothing.

## Why this exists

Teams hold documents in **Word**, *LibreOffice*, EPUB and markdown at the
same time. Getting between those formats is supposed to be boring. When it
is not boring, it is usually because something was silently dropped.

> A conversion you cannot inspect is a conversion you cannot trust.
>
> — every migration post-mortem, eventually

## Rollout schedule

| Phase | Owner | Starts | Status |
|-------|-------|-------:|--------|
| Inventory | Priya | 2026-01-06 | done |
| Pilot | Sam | 2026-02-17 | in progress |
| Migration | Ops | 2026-04-01 | not started |
| Decommission | Ops | 2026-09-15 | not started |

## What each team has to do

1. Export everything from the old system.
2. Convert it:
   - `.docx` and `.odt` to markdown for the index
   - markdown back to `.docx` for anyone who needs to edit in Word
3. Spot-check 1 % of the output by hand.

### Checklist

- [x] Inventory complete
- [ ] Pilot signed off
- [ ] Rollback plan written

## Running a conversion

Convert a directory in one pass:

```bash
find . -name '*.docx' -print0 |
  xargs -0 -P8 -I{} ferrodoc {} -t gfm -o {}.md
```

The Python equivalent, for a pipeline that already has one:

```python
import ferrodoc
with open("report.docx", "rb") as f:
    text = ferrodoc.convert(f.read(), "docx", "gfm")
```

An indented code block, which is a different construct:

    ferrodoc -f markdown -t html handbook.md

## Awkward cases, on purpose

Inline code with special characters: `a | b`, `<div>`, `--flag`, and a
literal backtick: `` ` ``.

Text with entities &amp; symbols — an em dash, "curly quotes", an
ellipsis…, a non-breaking&nbsp;space, and unicode: café, naïve, Ω, 日本語.

~~Struck-through text~~ and text with a footnote.[^note]

A link with a title: [the spec](https://example.com/spec "Format spec").
A bare autolink: <https://example.com/status>. An email:
<ops@example.com>.

An image: ![the logo](logo.png "Project logo")

<div class="callout">

Raw HTML block, which not every target format can hold.

</div>

Nested quoting:

> Outer quote.
>
> > Inner quote, with a list:
> >
> > - one
> > - two

[^note]: Footnote bodies can contain **formatting** and a
    [link](https://example.com).

## Sign-off

Contact <ops@example.com> with questions. See the `LICENSE` file for
terms.
