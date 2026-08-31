# Third-party notices

ferrodoc is MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`). Three
files in it are somebody else's, all under the **BSD 3-Clause** licence,
and that licence asks for its notice in redistributions **in binary form**
as well as in source — so this file ships beside the binaries, the wheels
and the npm package, not only in the repository.

Neither John MacFarlane nor pandoc endorses this project. The third BSD
clause forbids saying otherwise, and it is a real constraint rather than
boilerplate.

## Pandoc's HTML template and default stylesheet

`crates/ferrodoc-html/templates/html5.html` and `styles.html`, verbatim
from pandoc 3.8.2.1. Pandoc as a whole is GPL-2.0-or-later, but its
`COPYRIGHT` dual-licenses everything in `data/templates`:

> Pandoc's templates (in `data/templates`) are dual-licensed as either
> GPL (v2 or higher, same as pandoc) or (at your option) the BSD 3-clause
> license.

Taken under the BSD-3 option. Full text and provenance:
`crates/ferrodoc-html/templates/LICENSE`.

## Skylighting's highlighting stylesheet

`crates/ferrodoc-html/styles/highlight.css` — the 65 lines pandoc appends
to a standalone page when it highlights code, which are
`styleToCss pygments`.

These come from skylighting, not from pandoc, and skylighting is three
packages licensed apart: the colours (`skylighting-core`) and the CSS
renderer (`skylighting-format-blaze-html`) are **BSD-3**; only the
wrapper is GPL-2, and it is GPL because it bundles KDE's syntax
definitions. ferrodoc reads none of those definitions — its highlighter
is `crates/ferrodoc-html/src/highlight.rs`, written for this project — so
what is taken here is BSD-3 throughout. Full text and provenance:
`crates/ferrodoc-html/styles/LICENSE`.

    Copyright (c) 2016-2022, John MacFarlane.

## Skylighting's LaTeX highlighting preamble

`crates/ferrodoc-latex/styles/highlight.tex` — the environments and token
macros pandoc writes into a standalone LaTeX document that contains a
highlighted code block, which are `styleToLaTeX pygments` from
`skylighting-format-latex` (**BSD-3**, (C) 2016-2022 John MacFarlane).

The same reasoning as the stylesheet above: BSD-3 throughout, because the
GPL in skylighting is on the wrapper that bundles KDE's syntax
definitions and ferrodoc reads none of them. Full text and provenance:
`crates/ferrodoc-latex/styles/LICENSE`.
