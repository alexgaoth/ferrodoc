# ferrodoc-latex

Writes LaTeX. Gated by `diff-latex` (fidelity) and, in CI, by `pdflatex`
actually compiling the output — see the root `CLAUDE.md`.

- **There is no reader and there will not be one.** A `.tex` file expands
  user-defined macros, so reading it means interpreting a language rather
  than parsing a format. `TODO.md` lists it under explicit non-goals.
- **The gate is fidelity, not agreement with pandoc.** Pandoc's own LaTeX
  round trip scores **0/11** on `corpus`, so matching its output would gate
  us on copying its losses — the same reason `diff-md` is scored this way.
  The absolute number is low because the format is lossy through pandoc's
  reader, not because the writer is; `COMPATIBILITY.md` lists what cannot
  survive.
- **The check that matters is `pdflatex`.** Output pandoc reads back but
  TeX refuses has missed the point of the crate. CI installs
  `texlive-latex-base` and compiles every corpus document; locally,
  `the_whole_corpus_of_shapes_stays_well_formed` checks environments and
  braces balance, which is the failure mode that makes TeX unreadable.
- **Ten special characters, in two groups.** Seven take a backslash
  (`# $ % & _ { }`). Three cannot: `\\` is a line break, and `\~`/`\^` are
  accents waiting for a letter — those need `\textbackslash{}`,
  `\textasciitilde{}` and `\textasciicircum{}`, each with the `{}` so a
  following space survives.
- **A URL is not escaped the way text is.** Escaping `~` or `&` inside
  `\href` changes the address; only `% # { }` are touched.
- **`\verb` picks a delimiter the code does not contain.** Assuming `|`
  stops compiling the moment a pipe appears in a code span, and `\texttt`
  is not a substitute because it escapes its argument.
- **`\setcounter` goes before `\def\labelenumi`.** Pandoc's reader takes
  the start value from the first directive it meets and stops looking, so
  the other order begins every list at 1. Measured both ways round.
- **A blank line inside a list item makes pandoc read the whole list as
  `DefaultStyle`.** Blocks are separated by a blank line and never
  followed by one, which is what `blocks()` exists to get right.
- The preamble is deliberately minimal — `graphicx`, `longtable`,
  `booktabs`, `hyperref` — because a preamble that loads more fails on the
  minimal TeX installations this is aimed at. `hyperref` loads last, as it
  asks. **`ulem` used to be in that list and is not in
  `texlive-latex-base`**, so the first CI run that actually compiled the
  output failed every document on a missing `ulem.sty`. `\sout` is
  defined from kernel primitives instead. Adding a `\usepackage` here is
  a decision to require a bigger TeX; check the package ships in base
  first.
