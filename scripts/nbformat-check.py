#!/usr/bin/env python3
"""Judge every notebook ferrodoc writes with something that is not ferrodoc.

`nbformat.validate` is Jupyter's own schema validator: it is what refuses a
notebook the Jupyter server would not open. Run it over a notebook written
from each corpus document, plus one written from markdown, so the
`markdown -> ipynb` path is judged too.

    python3 scripts/nbformat-check.py corpus/ipynb-handmade/*.ipynb

Needs `pip install nbformat` and a built `ferrodoc` binary; both are what
CI installs.
"""

import pathlib
import subprocess
import sys
import tempfile

import nbformat

FERRODOC = pathlib.Path("target/release/ferrodoc")


def written(source: pathlib.Path, out: pathlib.Path) -> pathlib.Path:
    subprocess.run(
        [str(FERRODOC), str(source), "-t", "ipynb", "-o", str(out)],
        check=True,
    )
    return out


def main(argv: list[str]) -> int:
    if not FERRODOC.exists():
        print(f"{FERRODOC} is not built: cargo build --release -p ferrodoc", file=sys.stderr)
        return 2
    failures = 0
    with tempfile.TemporaryDirectory() as tmp:
        sources = [pathlib.Path(a) for a in argv]
        # One document that is not a notebook, so the writer is judged on
        # the path a user actually takes into this format.
        prose = pathlib.Path(tmp) / "prose.md"
        prose.write_text("# Title\n\nA paragraph with *emphasis*.\n\n- one\n- two\n")
        sources.append(prose)
        for source in sources:
            out = pathlib.Path(tmp) / (source.stem + ".written.ipynb")
            try:
                notebook = nbformat.read(written(source, out), as_version=4)
                nbformat.validate(notebook)
            except Exception as error:  # noqa: BLE001 - report and keep going
                failures += 1
                print(f"INVALID {source}: {error}")
                continue
            # nbformat 4.5 is what this writer claims to emit; the
            # validator accepts 4.0 too, so the version is checked here.
            assert (notebook.nbformat, notebook.nbformat_minor) == (4, 5), notebook.nbformat_minor
            assert all(cell.id for cell in notebook.cells), "a cell has no id"
            print(f"valid   {source} ({len(notebook.cells)} cells)")
    print(f"{len(sources) - failures}/{len(sources)} accepted by nbformat {nbformat.__version__}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
