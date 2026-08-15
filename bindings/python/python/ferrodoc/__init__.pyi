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

    Returns `str` for a text format and `bytes` for `docx`. Input formats:
    `markdown` (`commonmark`, `md`), `gfm` (`markdown_github`), `html`
    (`htm`), `docx`, `json`. Those plus `plain` (`text`, `txt`) as output.
    """

def formats() -> list[str]:
    """Every format name `convert` accepts."""
