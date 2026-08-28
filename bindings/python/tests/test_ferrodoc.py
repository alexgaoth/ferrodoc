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
    # Drop the spaces rather than collapse them: a pipe table's cells are
    # padded to their column, so `| a | b |` comes back `| a   | b   |`.
    # `replace("  ", " ")` is a single pass and leaves three spaces as
    # two, which is why this read as a lost table for four days.
    assert "|a|b|" in back.replace(" ", "")
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


def test_every_named_format_converts_in_the_direction_it_claims():
    # `formats()` is the union, and iterating it in one direction fails on
    # the first format that only goes the other way. That is exactly what
    # happened: `pandoc_markdown` is read-only, and this test could not
    # run at all for two versions because the wheel would not build, so
    # nothing said so.
    readable = ferrodoc.read_formats()
    writable = ferrodoc.write_formats()
    assert set(readable) | set(writable) == set(ferrodoc.formats())
    assert "pandoc_markdown" in readable and "pandoc_markdown" not in writable
    assert "plain" in writable and "plain" not in readable

    # Every writable format, in the type it should come back as. The
    # three zip formats are bytes and everything else is str — including
    # `ipynb`, which is JSON.
    for name in writable:
        out = ferrodoc.convert("x", "markdown", name)
        want = bytes if name in ("docx", "odt", "epub") else str
        assert isinstance(out, want), f"{name} came back as {type(out).__name__}"
    for name in readable:
        ferrodoc.convert(ferrodoc.convert("x", "markdown", "json"), "json", "markdown")
        if name in ("markdown", "commonmark", "md", "gfm", "markdown_github",
                    "pandoc_markdown", "html", "htm"):
            ferrodoc.convert("x", name, "markdown")


def test_the_error_survives_a_process_boundary():
    # The audience for this binding runs a pool over a directory. If the
    # exception cannot be pickled, one corrupt document does not raise —
    # it breaks the pool, and the batch dies instead of skipping a file.
    import pickle

    revived = pickle.loads(pickle.dumps(ferrodoc.ConversionError("boom")))
    assert isinstance(revived, ferrodoc.ConversionError)
    assert ferrodoc.ConversionError.__module__ == "ferrodoc", (
        "the class has to name an importable module or pickle cannot find it"
    )


def test_a_buffer_is_accepted_and_a_wrong_type_says_what_it_got():
    # An mmap or a socket read is a memoryview, and a caller should not
    # have to copy it first to satisfy a rule with no reason behind it.
    for data in (b"# T\n", bytearray(b"# T\n"), memoryview(b"# T\n")):
        # **This tracks the published `ferrodoc`, not the tree it sits
        # in**: `Cargo.toml` here depends on `ferrodoc = "0.7"` from
        # crates.io so the sdist resolves for anyone building it, so this
        # binding is one release behind the CLI by construction. On
        # 2026-08-28 `markdown` became pandoc's dialect in both
        # directions, which gives a heading pandoc's identifier — so when
        # the dependency is bumped past 0.7 this becomes
        # `<h1 id="t">T</h1>`, and the C and wasm bindings, which depend
        # by path, already say so.
        assert ferrodoc.convert(data, "markdown", "html") == "<h1>T</h1>\n"
    with pytest.raises(ValueError, match="not int"):
        ferrodoc.convert(42, "markdown", "html")


def test_the_gil_is_released_so_threads_actually_overlap():
    # The reason the binding is worth having over a subprocess: a pool of
    # threads converts in parallel. With the GIL held across a conversion,
    # N of them take N times one — that is the shape being tested, and the
    # only shape that can be tested portably.
    #
    # Both bounds here have to survive a shared CI runner. The worker
    # count follows the machine, because eight workers on four cores
    # cannot overlap eightfold however well the binding behaves: an
    # earlier version fixed at eight measured 2.45 of 8.0 here and 4.89 of
    # 8.0 on a smaller runner, and failed a threshold tuned to this
    # machine. And the document is large on purpose: at around 100 KiB the
    # pool's own overhead is a big share of the time.
    import concurrent.futures
    import os
    import time

    workers = min(4, os.cpu_count() or 1)
    if workers < 2:
        pytest.skip("one core cannot overlap anything")
    big = MARKDOWN * 8000

    ferrodoc.convert(big, "gfm", "docx")  # warm
    start = time.perf_counter()
    ferrodoc.convert(big, "gfm", "docx")
    alone = time.perf_counter() - start

    start = time.perf_counter()
    with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as pool:
        list(pool.map(lambda _: ferrodoc.convert(big, "gfm", "docx"), range(workers)))
    together = time.perf_counter() - start

    # Serial is `workers`; anything meaningfully under it is overlap the
    # GIL would have prevented.
    assert together < alone * workers * 0.85, (
        f"{workers} conversions took {together:.3f}s against {alone:.3f}s for one, "
        f"a ratio of {together / alone:.2f} where serial is {workers}.0; "
        "the GIL is being held across the conversion"
    )
