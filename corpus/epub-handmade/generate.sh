#!/usr/bin/env bash
# Books in shapes pandoc's own EPUB writer never emits.
#
# `corpus/epub` is pandoc's output, so a gate over it cannot fail on
# anything pandoc's writer does not produce — and pandoc's writer produces
# exactly one layout: EPUB 3, an `EPUB/` directory, chapters under `text/`,
# section divs, spine order matching file order.
#
# Real books are not all like that. These are hand-authored to be the
# things that layout hides: EPUB 2 with a `toc.ncx`, an `OEBPS/` directory,
# a package document at the archive root, a spine whose order is *not* the
# file order, a `linear="no"` cover, percent-encoded hrefs, and cross-file
# links. Every one of them exercises a rule in the reader that pandoc's
# own output leaves dormant.
set -euo pipefail
cd "$(dirname "$0")"
python3 generate.py
