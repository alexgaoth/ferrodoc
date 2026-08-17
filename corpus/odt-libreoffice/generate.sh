#!/usr/bin/env bash
# Regenerate the LibreOffice ODT corpus from the HTML sources it shares with
# `corpus/docx-libreoffice`. Requires `soffice` on PATH; run from anywhere.
#
# Why a second ODT corpus exists at all: `corpus/odt` is pandoc's own
# output, so `diff-odt` over it proves "ferrodoc reads what pandoc writes
# the way pandoc reads it" and cannot fail on a structure pandoc's ODT
# writer never emits — which is most of what a word processor emits.
# LibreOffice writes `text:p` where pandoc writes `text:h`, its own `L1`
# list styles with ten declared levels, `Table_20_Heading` cells,
# `text:sequence-decls`, and automatic paragraph styles that inherit
# through two links.
#
# The .odt bytes are LibreOffice's, and zip timestamps make them
# non-reproducible byte for byte, so what is checked is conformance, never
# a git diff.
set -euo pipefail
cd "$(dirname "$0")"

for source in ../docx-libreoffice/src/*.html; do
    name="$(basename "$source" .html)"
    soffice --headless --convert-to odt --outdir . "$source" >/dev/null
    echo "wrote $name.odt"
done
