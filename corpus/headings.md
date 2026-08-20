# Contents, numbering and the shapes that break them

A document whose whole job is headings: every rule `--toc` and
`--number-sections` follow has a case here, so breaking one drops this file
rather than passing silently.

## A second level

### A third level

#### A fourth level, past the contents depth

Pandoc's `--toc-depth` default is 3, so the heading above must not appear in
the contents while still being numbered in the body.

## Another second level

Text under it.

# A second first level
