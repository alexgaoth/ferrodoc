#!/usr/bin/env bash
# Write the compression-ratio fixtures 0.8's resource contract has to
# refuse, to ~/.cache/ferrodoc-bombs. They are generated rather than
# committed for the same reason corpus/bench is: they are a megabyte of
# input that expands to gigabytes, and generating them is cheaper than
# storing them — and a file this shaped in a repository is a nuisance to
# everyone who clones it.
#
#   bash corpus/bombs/generate.sh
#
# **Run the readers against these under a cap.** Uncapped, each one takes
# well over a gigabyte:
#
#   ( ulimit -v 3000000; ferrodoc ~/.cache/ferrodoc-bombs/ratio.docx -f docx -t plain )
#
# Measured 2026-08-27, before any bound existed:
#
#   ratio.docx   458 KB on disk -> 1.28 GB peak RSS   2,900x
#   ratio.epub   218 KB on disk -> 1.18 GB peak RSS   5,400x
#
# `verify.sh` gates peak RSS at **80x input**. Neither of these is in the
# corpus that gate runs on, which is why the bound has been stated and
# not defended.
set -euo pipefail
out="${1:-$HOME/.cache/ferrodoc-bombs}"
mkdir -p "$out"

python3 - "$out" <<'PYEOF'
import os, sys, zipfile
out = sys.argv[1]

body = b'<w:p><w:r><w:t>' + b'a' * 36 + b'</w:t></w:r></w:p>'
doc = (b'<?xml version="1.0"?><w:document '
       b'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">'
       b'<w:body>' + body * 2_000_000 + b'</w:body></w:document>')
path = os.path.join(out, 'ratio.docx')
with zipfile.ZipFile(path, 'w', zipfile.ZIP_DEFLATED, compresslevel=9) as z:
    z.writestr('[Content_Types].xml',
               '<?xml version="1.0"?><Types '
               'xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>')
    z.writestr('word/document.xml', doc)
print(f'{path}: {os.path.getsize(path)//1024} KB -> {len(doc)//1048576} MB '
      f'({len(doc)//os.path.getsize(path)}x)')

xhtml = (b'<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><body>'
         + (b'<p>' + b'a' * 44 + b'</p>') * 1_500_000 + b'</body></html>')
path = os.path.join(out, 'ratio.epub')
with zipfile.ZipFile(path, 'w', zipfile.ZIP_DEFLATED, compresslevel=9) as z:
    z.writestr('mimetype', 'application/epub+zip')
    z.writestr('META-INF/container.xml',
               '<?xml version="1.0"?><container '
               'xmlns="urn:oasis:names:tc:opendocument:xmlns:container">'
               '<rootfiles><rootfile full-path="c.opf"/></rootfiles></container>')
    z.writestr('c.opf',
               '<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" '
               'version="3.0"><metadata/><manifest>'
               '<item id="a" href="a.xhtml" media-type="application/xhtml+xml"/>'
               '</manifest><spine><itemref idref="a"/></spine></package>')
    z.writestr('a.xhtml', xhtml)
print(f'{path}: {os.path.getsize(path)//1024} KB -> {len(xhtml)//1048576} MB '
      f'({len(xhtml)//os.path.getsize(path)}x)')
PYEOF
