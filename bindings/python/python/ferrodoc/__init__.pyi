from typing import Literal, overload

__version__: str

class ConversionError(ValueError):
    """A document could not be read as the format it was said to be, or
    could not be written as the format asked for."""

TextFormat = Literal["markdown", "commonmark", "md", "gfm", "html", "json", "plain", "text", "txt"]
BinaryFormat = Literal["docx"]
Format = Literal[TextFormat, BinaryFormat]

@overload
def convert(data: str | bytes, from_format: Format, to_format: TextFormat) -> str: ...
@overload
def convert(data: str | bytes, from_format: Format, to_format: BinaryFormat) -> bytes: ...

def formats() -> list[str]: ...
