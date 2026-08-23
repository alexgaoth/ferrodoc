"""Convert documents between markdown, HTML and DOCX, in this process.

The compiled module does the work; this package exists to carry the type
stubs and to keep the import name stable if the extension is ever split.
"""

from ._ferrodoc import (
    ConversionError,
    __version__,
    convert,
    formats,
    read_formats,
    write_formats,
)

__all__ = [
    "ConversionError",
    "__version__",
    "convert",
    "formats",
    "read_formats",
    "write_formats",
]
