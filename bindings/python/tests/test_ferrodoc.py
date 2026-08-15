"""Tests for the ferrodoc Python bindings.

These check the binding, not the converter — the converter is checked
document by document against pandoc by the Rust harness. What can only go
wrong here is the boundary: which Python type comes back, what happens to
bytes that are not the format they claim to be, and whether an error
arrives as an exception rather than as a crash.

    pip install dist/*.whl && python -m pytest tests/
"""

import json
import pathlib

import pytest

import ferrodoc

MARKDOWN = "# Title\n\nHello *world*.\n\n| a | b |\n|---|---|\n| 1 | 2 |\n"
CORPUS = pathlib.Path(__file__).resolve().parents[3] / "corpus"


def test_text_output_is_str_and_binary_output_is_bytes():
    # The whole reason `convert` inspects the target format: a caller
    # writing markdown to a file wants str, and a caller writing a .docx
    # wants bytes. Returning bytes for both would make every text caller
    # decode, and returning str for docx would corrupt it.
    assert isinstance(ferrodoc.convert(MARKDOWN, "gfm", "html"), str)
    assert isinstance(ferrodoc.convert(MARKDOWN, "gfm", "docx"), bytes)


def test_str_and_bytes_input_agree():
    from_text = ferrodoc.convert(MARKDOWN, "gfm", "html")
    from_bytes = ferrodoc.convert(MARKDOWN.encode(), "gfm", "html")
    assert from_text == from_bytes


def test_a_table_survives_markdown_to_docx_and_back():
    # The path the binding exists for. A table is the structure most
    # easily lost, and the one people notice.
    docx = ferrodoc.convert(MARKDOWN, "gfm", "docx")
    back = ferrodoc.convert(docx, "docx", "gfm")
    assert "| a | b |" in back.replace("  ", " ")
    assert "Title" in back


def test_the_ast_is_reachable_through_the_json_format():
    # No second API to keep in step: the pandoc AST is a format.
    ast = json.loads(ferrodoc.convert(MARKDOWN, "gfm", "json"))
    assert [block["t"] for block in ast["blocks"]] == ["Header", "Para", "Table"]


@pytest.mark.skipif(not CORPUS.exists(), reason="corpus is not beside the wheel")
def test_a_document_another_program_wrote():
    # Not one of ours: LibreOffice Writer produced this one.
    document = CORPUS / "docx-libreoffice" / "report.docx"
    markdown = ferrodoc.convert(document.read_bytes(), "docx", "gfm")
    assert "# Quarterly Report" in markdown
    assert "| Region | Growth |" in markdown
    assert "[the appendix](https://example.com/appendix)" in markdown


def test_an_unknown_format_names_the_ones_that_exist():
    with pytest.raises(ValueError) as caught:
        ferrodoc.convert(MARKDOWN, "markdown", "pdf")
    assert "pdf" in str(caught.value)
    assert "docx" in str(caught.value), "the message should list what is available"


def test_bad_input_raises_rather_than_crashing():
    with pytest.raises(ferrodoc.ConversionError):
        ferrodoc.convert(b"this is not a zip archive", "docx", "gfm")
    with pytest.raises(ferrodoc.ConversionError):
        ferrodoc.convert(b"{ not json", "json", "html")


def test_data_has_to_be_str_or_bytes():
    with pytest.raises(ValueError):
        ferrodoc.convert(42, "markdown", "html")


def test_conversion_error_is_a_value_error():
    # So a caller can catch ValueError without knowing this package.
    assert issubclass(ferrodoc.ConversionError, ValueError)


def test_empty_input_is_an_empty_document_not_an_error():
    # A newline, which is exactly what `pandoc -f commonmark -t html`
    # writes for the same input — not an empty string, and not an error.
    assert ferrodoc.convert("", "markdown", "html") == "\n"


def test_formats_are_the_ones_convert_accepts():
    for name in ferrodoc.formats():
        ferrodoc.convert("x", "markdown", name)


def test_the_gil_is_released_so_threads_actually_overlap():
    # The reason the binding is worth having over a subprocess: a pool of
    # threads converts in parallel. If the GIL were held, wall-clock time
    # would be the sum rather than the max.
    import concurrent.futures
    import time

    big = MARKDOWN * 2000
    start = time.perf_counter()
    ferrodoc.convert(big, "gfm", "docx")
    alone = time.perf_counter() - start

    start = time.perf_counter()
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        list(pool.map(lambda _: ferrodoc.convert(big, "gfm", "docx"), range(4)))
    together = time.perf_counter() - start

    assert together < alone * 3.5, (
        f"four conversions took {together:.3f}s against {alone:.3f}s for one; "
        "that is serial, so the GIL is being held across the conversion"
    )
