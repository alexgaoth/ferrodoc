#!/usr/bin/env bash
# Regenerate the ODT corpus from the DOCX corpus's committed sources with
# pandoc. Requires pandoc 3.8.x on PATH; run from anywhere.
#
# The sources are shared with `corpus/docx/src` on purpose: the same
# documents through two office writers make a mismatch attributable to the
# reader rather than to the fixture.
#
# The .odt bytes are pandoc's own output, so `diff-odt` over this directory
# proves "ferrodoc reads what pandoc writes the way pandoc reads it" and
# nothing more — `corpus/odt-libreoffice` is what shows the reader handles a
# document pandoc did not author. Zip timestamps make the bytes
# non-reproducible, so what is checked is conformance, never a git diff.
set -euo pipefail
cd "$(dirname "$0")"

src=../docx/src

for f in "$src"/*.md; do
    name=$(basename "$f" .md)
    (cd "$src" && pandoc "$name.md" -o "$OLDPWD/$name.odt")
done
(cd "$src" && pandoc tables.html -o "$OLDPWD/tables.odt")

for f in ../*.md; do
    name=$(basename "$f" .md)
    pandoc "$f" -o "corpus-$name.odt"
done

# Spec examples in chunks of 30, concatenated with blank lines.
python3 - <<'PY'
import json, subprocess
spec = json.load(open('../commonmark-spec-0.31.2.json'))
mds = [e['markdown'] for e in spec]
for i in range(0, len(mds), 30):
    md = '\n\n'.join(mds[i:i+30])
    subprocess.run(['pandoc', '-f', 'markdown', '-o', f'spec-{i//30:02d}.odt'],
                   input=md, text=True, check=True)
PY
