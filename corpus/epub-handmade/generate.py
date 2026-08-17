"""Build the hand-authored EPUB corpus. See generate.sh for why it exists."""
import zipfile

# EPUB 3 content documents are HTML5; EPUB 2 content documents are XHTML
# 1.1, and epubcheck holds each to its own doctype.
XHTML = """<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>{title}</title></head>
<body>
{body}
</body></html>"""

XHTML11 = """<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" \
"http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>{title}</title></head>
<body>
{body}
</body></html>"""


NAV = """<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>Contents</title></head><body>
<nav epub:type="toc" id="toc"><ol>{items}</ol></nav>
</body></html>"""

def book(name, opf_path, files, manifest, spine, version="3.0", ncx=None, nav=None):
    """Assemble one EPUB. `files` is path -> text; the mimetype entry is
    stored uncompressed and first, as every reading system expects."""
    opf_dir = opf_path.rsplit("/", 1)[0] + "/" if "/" in opf_path else ""
    container = (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<container version="1.0" '
        'xmlns="urn:oasis:names:tc:opendocument:xmlns:container">\n'
        f'  <rootfiles><rootfile full-path="{opf_path}" '
        'media-type="application/oebps-package+xml"/></rootfiles>\n'
        "</container>\n"
    )
    opf = (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        f'<package xmlns="http://www.idpf.org/2007/opf" version="{version}" '
        'unique-identifier="bookid">\n'
        '  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">\n'
        f"{manifest['metadata']}"
        "  </metadata>\n"
        f"  <manifest>\n{manifest['items']}  </manifest>\n"
        f"  <spine{' toc=\"ncx\"' if ncx else ''}>\n{spine}  </spine>\n"
        "</package>\n"
    )
    with zipfile.ZipFile(name, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr(
            zipfile.ZipInfo("mimetype"), "application/epub+zip",
            compress_type=zipfile.ZIP_STORED,
        )
        z.writestr("META-INF/container.xml", container)
        z.writestr(opf_path, opf)
        if ncx:
            z.writestr(opf_dir + "toc.ncx", ncx)
        if nav:
            z.writestr(opf_dir + "nav.xhtml", nav)
        for path, text in files.items():
            z.writestr(opf_dir + path, text)


# --- 1. EPUB 2, OEBPS layout, spine order != file order ------------------
# A reader that walked the zip, or sorted by name, would put these three
# chapters in the wrong order. Only the spine says which is which.
book(
    "reversed-spine.epub",
    "OEBPS/package.opf",
    {
        "c-last.xhtml": XHTML11.format(title="Last", body="<h1>Gamma</h1><p>Third.</p>"),
        "b-middle.xhtml": XHTML11.format(title="Middle", body="<h1>Beta</h1><p>Second.</p>"),
        "a-first.xhtml": XHTML11.format(title="First", body="<h1>Alpha</h1><p>First.</p>"),
    },
    {
        "metadata": (
            "    <dc:title>Out of order</dc:title>\n"
            "    <dc:creator>A Binder</dc:creator>\n"
            "    <dc:language>en</dc:language>\n"
            '    <dc:identifier id="bookid">urn:uuid:11111111-2222-3333-4444-555555555551</dc:identifier>\n'
        ),
        "items": (
            '    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>\n'
            '    <item id="a" href="a-first.xhtml" media-type="application/xhtml+xml"/>\n'
            '    <item id="b" href="b-middle.xhtml" media-type="application/xhtml+xml"/>\n'
            '    <item id="c" href="c-last.xhtml" media-type="application/xhtml+xml"/>\n'
        ),
    },
    spine=(
        '    <itemref idref="a"/>\n'
        '    <itemref idref="b"/>\n'
        '    <itemref idref="c"/>\n'
    ),
    version="2.0",
    ncx=(
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">\n'
        '<head><meta name="dtb:uid" '
        'content="urn:uuid:11111111-2222-3333-4444-555555555551"/></head>\n'
        "<docTitle><text>Out of order</text></docTitle>\n"
        "<navMap>\n"
        '  <navPoint id="n1" playOrder="1"><navLabel><text>Alpha</text></navLabel>'
        '<content src="a-first.xhtml"/></navPoint>\n'
        '  <navPoint id="n2" playOrder="2"><navLabel><text>Beta</text></navLabel>'
        '<content src="b-middle.xhtml"/></navPoint>\n'
        '  <navPoint id="n3" playOrder="3"><navLabel><text>Gamma</text></navLabel>'
        '<content src="c-last.xhtml"/></navPoint>\n'
        "</navMap></ncx>\n"
    ),
)

# --- 2. A cover outside the reading order, and a shared heading id -------
# Both chapters define `#intro`. Concatenated without prefixing they would
# collide; the cover is `linear="no"` and contributes nothing at all.
book(
    "cover-and-collisions.epub",
    "OEBPS/content.opf",
    {
        "cover.xhtml": XHTML.format(title="Cover", body="<h1>Cover Art</h1>"),
        "one.xhtml": XHTML.format(
            title="One",
            body='<h2 id="intro">Intro</h2><p>See <a href="two.xhtml#intro">the other intro</a>.</p>',
        ),
        "two.xhtml": XHTML.format(
            title="Two",
            body='<h2 id="intro">Intro</h2><p>And <a href="#intro">back to this one</a>.</p>',
        ),
    },
    {
        "metadata": (
            "    <dc:title>Collisions</dc:title>\n"
            "    <dc:language>en</dc:language>\n"
            '    <dc:identifier id="bookid">urn:uuid:11111111-2222-3333-4444-555555555552</dc:identifier>\n'
            '    <meta property="dcterms:modified">2026-01-01T00:00:00Z</meta>\n'
        ),
        "items": (
            '    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>\n'
            '    <item id="cover" href="cover.xhtml" media-type="application/xhtml+xml"/>\n'
            '    <item id="one" href="one.xhtml" media-type="application/xhtml+xml"/>\n'
            '    <item id="two" href="two.xhtml" media-type="application/xhtml+xml"/>\n'
        ),
    },
    spine=(
        '    <itemref idref="cover" linear="no"/>\n'
        '    <itemref idref="one"/>\n'
        '    <itemref idref="two"/>\n'
    ),
    # The cover is linear="no", and EPUB 3 requires non-linear content to
    # be reachable from somewhere — the navigation document is where.
    nav=NAV.format(
        items='<li><a href="cover.xhtml">Cover</a></li>'
        '<li><a href="one.xhtml">One</a></li><li><a href="two.xhtml">Two</a></li>'
    ),
)

# --- 3. Package document at the archive root, percent-encoded href -------
# epubcheck warns (PKG-010) that the space in the file name may trouble old
# reading systems. That is the point of the fixture: the space is what
# makes the manifest href and the archive entry differ.
# The base directory is "" here, which is the case a reader that always
# strips a directory gets wrong, and a space in a file name is escaped in
# the manifest and literal in the archive.
book(
    "root-package.epub",
    "book.opf",
    {
        "a chapter.xhtml": XHTML.format(
            title="Spaced",
            body="<h1>Spaced Out</h1><p>The file name has a space in it.</p>",
        ),
    },
    {
        "metadata": (
            "    <dc:title>At the root</dc:title>\n"
            "    <dc:language>en</dc:language>\n"
            '    <dc:identifier id="bookid">urn:uuid:11111111-2222-3333-4444-555555555553</dc:identifier>\n'
            '    <meta property="dcterms:modified">2026-01-01T00:00:00Z</meta>\n'
        ),
        "items": (
            '    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>\n'
            '    <item id="ch" href="a%20chapter.xhtml" media-type="application/xhtml+xml"/>\n'
        ),
    },
    spine='    <itemref idref="ch"/>\n',
    nav=NAV.format(items='<li><a href="a%20chapter.xhtml">Spaced Out</a></li>'),
)
