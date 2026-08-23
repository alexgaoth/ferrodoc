from typing import Union

__version__: str

class ConversionError(ValueError):
    """A document could not be read as the format it was said to be, or
    could not be written as the format asked for."""

# Every spelling `convert` accepts, which is what `Format::parse` accepts.
# Written as plain `str` rather than a `Literal` union: a format name
# usually arrives from argv, a config file or a MIME lookup, and a stub
# that rejected `str` would make the common call site fail to type-check
# for no benefit.
Format = str

def convert(
    data: Union[str, bytes, bytearray, memoryview],
    from_format: Format,
    to_format: Format,
) -> Union[str, bytes]:
    """Convert a document from one format to another.

    Returns `str` for a text format and `bytes` for the packaged ones —
    `docx`, `odt`, `epub` and `ipynb`. Ask `read_formats()` and
    `write_formats()` rather than reading a list here that goes stale:
    this one said `docx, json` for two versions after `odt`, `epub`,
    `ipynb`, `latex`, `rst` and `asciidoc` existed.
    """

def formats() -> list[str]:
    """Every format name `convert` knows, in either direction.

    Not all of them both ways: `pandoc_markdown` is read-only and
    `plain`, `latex`, `rst` and `asciidoc` are write-only.
    """

def read_formats() -> list[str]:
    """The formats a document can be converted from."""

def write_formats() -> list[str]:
    """The formats a document can be converted to."""
