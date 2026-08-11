#!/usr/bin/env bash
# Regenerate the DOCX corpus from its committed sources with pandoc.
# Requires pandoc 3.8.x on PATH; run from anywhere.
#
# The .docx bytes are pandoc's own output (zip timestamps make them
# non-reproducible byte-for-byte, so conformance is what is checked, not a
# git diff). Two variants are then rewritten by a scripted transformation
# to exercise section page sizes and over-wide table grids, which pandoc's
# writer never emits but Word documents do.
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
# Section numbering emits w:tab between the number and the heading text.
(cd src && pandoc --number-sections meta-and-defs.md -o ../meta-and-defs.docx)
(cd src && pandoc tables.html -o ../tables.docx)

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

# Word-style variants: an explicit page size (so the text width is not the
# 9360-twip default) and a grid whose columns sum past the text width (so
# the width fractions must be normalized).
python3 - <<'PY'
import re, shutil, zipfile

def rewrite(src, dst, transform):
    shutil.copy(src, dst)
    zin = zipfile.ZipFile(src)
    items = [(i, zin.read(i.filename)) for i in zin.infolist()]
    zin.close()
    with zipfile.ZipFile(dst, 'w', zipfile.ZIP_DEFLATED) as zout:
        for info, data in items:
            if info.filename == 'word/document.xml':
                data = transform(data.decode()).encode()
            zout.writestr(info, data)

SECT = ('<w:sectPr><w:pgSz w:w="15840" w:h="12240"/>'
        '<w:pgMar w:left="720" w:right="720" w:gutter="360"/></w:sectPr>')

def set_page_size(xml):
    if '<w:sectPr' in xml:
        return re.sub(r'<w:sectPr.*?</w:sectPr>', SECT, xml, flags=re.S)
    return xml.replace('</w:body>', SECT + '</w:body>')

def widen_grid(xml):
    return re.sub(r'(<w:gridCol w:w=")(\d+)(")',
                  lambda m: m.group(1) + str(int(m.group(2)) * 3) + m.group(3), xml)

def derived_style(xml):
    # Re-style the block quotes with a custom style that only inherits its
    # meaning, so the basedOn chain has to be walked to recognize them.
    return xml.replace('<w:pStyle w:val="BlockText" />',
                       '<w:pStyle w:val="MyQuote" />')

rewrite('tables.docx', 'tables-wide-page.docx', set_page_size)
rewrite('tables.docx', 'tables-overwide-grid.docx', widen_grid)

# A custom style that means "block quote" only through its basedOn chain.
import zipfile as _zip
shutil.copy('corpus-nested-structures.docx', 'derived-styles.docx')
zin = _zip.ZipFile('corpus-nested-structures.docx')
items = [(i, zin.read(i.filename)) for i in zin.infolist()]
zin.close()
STYLE = ('<w:style w:type="paragraph" w:styleId="MyQuote">'
         '<w:name w:val="My Quote"/><w:basedOn w:val="BlockText"/></w:style>')
with _zip.ZipFile('derived-styles.docx', 'w', _zip.ZIP_DEFLATED) as zout:
    for info, data in items:
        if info.filename == 'word/document.xml':
            data = derived_style(data.decode()).encode()
        elif info.filename == 'word/styles.xml':
            data = data.decode().replace('</w:styles>', STYLE + '</w:styles>').encode()
        zout.writestr(info, data)
PY
