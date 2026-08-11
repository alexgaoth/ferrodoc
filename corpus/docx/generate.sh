#!/usr/bin/env bash
# Regenerate the DOCX corpus from markdown sources with pandoc.
# Requires pandoc 3.8.x on PATH; run from anywhere.
set -euo pipefail
cd "$(dirname "$0")"

# 1x1 PNG referenced by notes-and-images.md
python3 - <<'PY'
import struct, zlib
def chunk(t, d):
    c = struct.pack('>I', len(d)) + t + d
    return c + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
png = (b'\x89PNG\r\n\x1a\n'
       + chunk(b'IHDR', struct.pack('>IIBBBBB', 1, 1, 8, 2, 0, 0, 0))
       + chunk(b'IDAT', zlib.compress(b'\x00\xff\x00\x00'))
       + chunk(b'IEND', b''))
open('src/logo.png', 'wb').write(png)
PY

for f in src/*.md; do
    name=$(basename "$f" .md)
    (cd src && pandoc "$name.md" -o "../$name.docx")
done
for f in ../*.md; do
    name=$(basename "$f" .md)
    pandoc "$f" -o "corpus-$name.docx"
done

# Spec examples in chunks of 30, concatenated with blank lines.
python3 - <<'PY'
import json, subprocess
spec = json.load(open('../commonmark-spec-0.31.2.json'))
mds = [e['markdown'] for e in spec]
for i in range(0, len(mds), 30):
    md = '\n\n'.join(mds[i:i+30])
    subprocess.run(['pandoc', '-f', 'markdown', '-o', f'spec-{i//30:02d}.docx'],
                   input=md, text=True, check=True)
PY
