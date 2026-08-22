# Releasing

Four artefacts go to three registries, and they have an order. This file
is the checklist, and every command in it was run against 0.2.0 on
2026-08-22 — the results are recorded beside each one, including the one
that cannot succeed yet and why.

**The order is not a preference.** `bindings/python` depends on
`ferrodoc` from *crates.io*, not by path, so that maturin's source
distribution — which contains only that directory — resolves for anyone
building it. Until crates.io has the version the binding asks for, the
wheel cannot be built at all:

```console
$ maturin build --release -m bindings/python/Cargo.toml
error: failed to select a version for the requirement `ferrodoc = "^0.2"`
candidate versions found which didn't match: 0.1.0
```

So: **crates.io, then the GitHub release**, which is what triggers PyPI
and npm.

## 0. Before anything

```sh
./scripts/verify.sh          # must exit 0 — tests, clippy, wasm32, 23 gates
./scripts/verify.sh --wasm   # the npm package, incl. headless Chrome
./scripts/verify.sh --c      # the C ABI under valgrind
./scripts/verify.sh --fuzz   # 500k mutations, after any reader change
gh run list --limit 5        # main must be green
```

A red `main` is a release blocker on its own: three of these jobs failed
for two days in August 2026 and each failure was invisible locally.

Check the version is the same everywhere. It lives in four files and a
bump is not a search-and-replace:

```sh
grep -rn '^version' Cargo.toml bindings/python/Cargo.toml
grep -n '"version"' bindings/wasm/package.json
grep -n 'ferrodoc = ' bindings/python/Cargo.toml   # the caret must match
```

## 1. Dress rehearsal — build every artefact, publish none

```sh
cargo publish --dry-run --workspace
```

Twelve crates package and verify: `ferrodoc-ast`, `-asciidoc`, `-docx`,
`-markdown`, `-html`, `-epub`, `-ipynb`, `-latex`, `-odt`, `-rst`,
`-text`, and `ferrodoc`. `ferrodoc-harness` is `publish = false` — it
shells out to pandoc and reads the corpus.

```sh
./bindings/wasm/build.sh && (cd bindings/wasm && npm pack)
```

Writes `bindings/wasm/ferrodoc-0.2.0.tgz`: five files, `js/ferrodoc.wasm`
at 1,855,728 bytes. **Check that number.** A build that fell back to the
CLI stub produces a module around 31 KB, and it has been shipped that way
once — the package would install and convert nothing.

```sh
maturin build --release -m bindings/python/Cargo.toml
```

Fails until step 2 has happened. That is the ordering above, not a
problem to be worked around: patching the dependency to a path would
break the sdist, and the sdist building on its own is what proves the
wheel is self-contained.

## 2. crates.io — first, and as one command

```sh
cargo login                        # or CARGO_REGISTRY_TOKEN
cargo publish --workspace
```

**Never a loop over `cargo publish -p`.** A loop keeps going after a
failure, so one error becomes six half-published crates and a version
number that can never be reused.

Then confirm the registry actually has it, rather than that the command
exited 0:

```sh
cargo add ferrodoc --dry-run
maturin build --release -m bindings/python/Cargo.toml   # now resolves
```

## 3. The GitHub release — triggers PyPI and npm

One-time setup, both of which are the owner's and cannot be delegated:

- **PyPI trusted publishing.** A pending publisher naming this repository,
  the workflow file `wheels.yml` and the environment `pypi`. The
  environment already exists. No token lives in this repository.
- **npm.** `gh secret set NPM_TOKEN` with an automation token. `npm
  publish` runs with `--provenance`, which needs the release to come from
  a workflow.

Then:

```sh
git tag -a v0.2.0 -m 'ferrodoc 0.2.0'
git push origin v0.2.0
gh release create v0.2.0 --generate-notes --notes-file CHANGELOG.md
```

`release.yml` builds and attaches the CLI binaries; `wheels.yml` builds
the four wheels and the sdist and publishes them to PyPI on
`release: published`.

## 4. Installing is not building

The exit test for a release is a **clean machine**, one per platform:

```sh
cargo install ferrodoc && ferrodoc --version
pip install ferrodoc && python -c "import ferrodoc; print(ferrodoc.convert('# a', 'gfm', 'html'))"
npm install ferrodoc && node -e "import('ferrodoc').then(m => m.default()).then(f => console.log(f.convert('# a', 'gfm', 'html')))"
```

Every wheel is built, installed and tested on four platforms in CI
already. That is not the same as the package resolving from PyPI, and the
README made the stronger claim while `pip install ferrodoc` 404'd.

## Known state, 2026-08-22

- `cargo publish --dry-run --workspace`: **12/12 packaged and verified**,
  every one reporting 0.2.0.
- `npm pack`: **0.2.0, 689,147 bytes**, the real module inside.
- `maturin build`: **blocked on step 2**, as designed.
- The two tokens pasted into a chat transcript on 2026-08-20 must be
  **rotated before use**. They are in that transcript.
