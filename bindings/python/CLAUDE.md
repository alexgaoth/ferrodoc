# ferrodoc-python

The `ferrodoc` wheel on PyPI. Built by maturin, not by cargo.

## Commands

- `maturin build --release --out dist`,
  `pip3 install --user --force-reinstall dist/*.whl`, `python3 -m pytest
  tests/ -q`. pytest tests the *installed* wheel, so rebuild and reinstall
  after editing `src/lib.rs` or you are testing the old one.
- `maturin sdist --out dist` must build standalone; CI proves it from a
  fresh directory.

## Rules

- **This crate is outside the cargo workspace** (`exclude` in the root
  `Cargo.toml`), so `cargo test --workspace` needs no Python headers — and
  so it inherits none of the workspace lints. `unsafe_code = "forbid"` is
  declared here again for that reason; without it the guarantee the README
  makes does not hold in this directory.
- **Depends on `ferrodoc` from crates.io, never by path.** maturin's source
  distribution contains only this directory, so a path dependency resolves
  for nobody building the sdist.
- **`create_exception!` must name `ferrodoc`, not `_ferrodoc`.** Pickle
  resolves a class by `__module__`, `_ferrodoc` is not importable, and an
  unpicklable exception cannot cross a process boundary: a
  `ProcessPoolExecutor` over a directory of documents met one corrupt file
  and the whole pool died with `PicklingError` instead of raising
  `ConversionError`. That is this binding's main audience.
- `convert` returns `str` for a text format and `bytes` for DOCX, decided
  by `is_text`. Callers write markdown to a file and DOCX to a file; one
  return type would make one of them wrong.
- The GIL is released with `python.detach` (pyo3's name for `allow_threads`
  since 0.23) so a thread pool overlaps.
- **A timing threshold must scale with `os.cpu_count()`.** Eight workers
  cannot overlap eightfold on a four-core runner: a bound read off a
  16-core machine passed locally and failed CI at a ratio that proved the
  GIL *was* released. Check any such test under `taskset -c 0-1` too.
- The Rust module is `_ferrodoc`; the importable package is
  `python/ferrodoc/`, carrying `__init__.pyi` and `py.typed`. Keep the
  stub in step with `Format::parse`.
- Wheels are `abi3-py39`, so one per platform serves every Python from 3.9.
  CI builds them on Linux, both macOS architectures and Windows, and
  *installs and runs the tests against the installed wheel* — a wheel that
  builds but does not import is the failure that check exists for.
