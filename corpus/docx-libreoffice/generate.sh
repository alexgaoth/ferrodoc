#!/usr/bin/env bash
# Regenerate the LibreOffice DOCX corpus from its committed HTML sources.
# Requires `soffice` on PATH; run from anywhere.
#
# Why a second DOCX corpus exists at all: `corpus/docx` is pandoc's own
# output, so `diff-docx` over it proves "ferrodoc reads what pandoc writes
# the way pandoc reads it". It cannot fail on a structure pandoc's writer
# never emits — and pandoc's writer emits almost none of what a word
# processor does. LibreOffice writes `Heading1`, `BodyText`,
# `TableContents` and `TableHeading` styles, its own `numbering.xml`,
# `w:tblLayout`, and a `w:sectPr` shaped differently from pandoc's.
#
# Converted in two steps, through ODT, so the second step is LibreOffice
# Writer's own DOCX export rather than its Writer/Web filter — the path a
# document actually takes when someone saves as .docx.
#
# The .docx bytes are LibreOffice's, and zip timestamps make them
# non-reproducible byte for byte, so what is checked is conformance, never
# a git diff.
set -euo pipefail
cd "$(dirname "$0")"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

for source in src/*.html; do
    name="$(basename "$source" .html)"
    soffice --headless --convert-to odt --outdir "$work" "$source" >/dev/null
    soffice --headless --convert-to 'docx:MS Word 2007 XML' --outdir "$work" \
        "$work/$name.odt" >/dev/null
    mv "$work/$name.docx" "./$name.docx"
    echo "wrote $name.docx"
done
