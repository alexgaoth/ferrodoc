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

**Expect to run this twice, and budget an hour.** crates.io allows a
burst of **5 brand-new crates** per account and then one per ten
minutes; new *versions* of crates that already exist get a burst of 30
and one per minute. 0.2.0 introduced six new crates, so the sixth is
refused with `429 Too Many Requests` naming the time to retry, and
`cargo publish --workspace` stops there rather than continuing. That is
the right behaviour and it is not a failed release: everything before it
is published and stays published.

Resuming is by name, and it is not the loop the rule above forbids —
each is a deliberate publish of one crate that is known to be missing:

```sh
cargo publish -p ferrodoc-epub     # the one the limit refused
cargo publish -p ferrodoc          # the facade, last, once its deps are up
```

Check what is actually on the registry rather than what the log said:

```sh
for c in ferrodoc ferrodoc-ast ferrodoc-markdown ferrodoc-html ferrodoc-text \
         ferrodoc-docx ferrodoc-odt ferrodoc-epub ferrodoc-latex ferrodoc-rst \
         ferrodoc-asciidoc ferrodoc-ipynb; do
  printf '%-20s %s\n' "$c" \
    "$(cargo search "$c" --limit 1 | sed -n "s/^$c = \"\(.*\)\".*/\1/p")"
done
```

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

First refresh the Python binding's lock, which still pins the *previous*
`ferrodoc` and can only be updated once step 2 has happened:

```sh
cargo update -p ferrodoc --manifest-path bindings/python/Cargo.toml
git add bindings/python/Cargo.lock && git commit -m 'chore: lock the published ferrodoc'
```

Then the tag and the release. The notes are the changelog's **section for
this version**, not the whole file — `--notes-file CHANGELOG.md` would put
the 0.1.0 history and an `Unreleased` heading in the release body:

```sh
awk '/^## 0\.2\.0/{f=1;next} /^## /{f=0} f' CHANGELOG.md > /tmp/notes.md
git tag -a v0.2.0 -m 'ferrodoc 0.2.0'
git push origin v0.2.0
gh release create v0.2.0 --title 'ferrodoc 0.2.0' --notes-file /tmp/notes.md
```

`release.yml` builds and attaches the CLI binaries and runs `npm publish
--provenance`; `wheels.yml` builds the four wheels and the sdist and
publishes them to PyPI. Both fire on `release: published`.

### Why a wheel, and why a workflow

Worth stating, because "publish to PyPI" sounds like one command and is
not:

- **The Python package is compiled Rust**, a pyo3 extension module. There
  is no pure-Python fallback, so a wheel is not an optimisation: without
  one, `pip install ferrodoc` downloads the sdist and tries to *compile*
  it, which needs a Rust toolchain the average Python user does not have.
  That install does not degrade, it fails.
- **A compiled extension is per-platform**, so four wheels: manylinux
  x86_64, macOS arm64, macOS x86_64 and Windows x64. They are `abi3`, so
  one wheel per platform covers Python 3.9 and every version above it —
  otherwise it would be four platforms times six interpreters.
- **Three of those four cannot be built here.** macOS and Windows wheels
  need macOS and Windows machines, which is what the runner matrix is.
- **PyPI is not given a token at all.** The upload uses trusted
  publishing: PyPI checks an OIDC claim that the upload came from this
  repository, this workflow file and the `pypi` environment, and rejects
  anything else. That is why the release event is the trigger and why
  there is no secret to leak. npm's `--provenance` works the same way and
  needs the workflow context for the same reason.

So the GitHub release is not ceremony around the publish — it *is* the
publish, for two of the three registries.

## 4. Installing is not building

The exit test for a release is a **clean machine**, one per platform:

```sh
cargo install ferrodoc && ferrodoc --version

python3 -m venv /tmp/v && /tmp/v/bin/pip install ferrodoc
/tmp/v/bin/python -c "import ferrodoc; print(ferrodoc.convert('# a', 'gfm', 'html'))"

mkdir /tmp/n && cd /tmp/n && npm init -y && npm install ferrodoc
node --input-type=module -e "
import { convert } from 'ferrodoc';
console.log(await convert('# a', 'gfm', 'html'));"
```

Run these as printed. The npm line here used to be
`import('ferrodoc').then(m => m.default())`, which throws
`TypeError: m.default is not a function` — the package exports a **named,
async** `convert`, and nothing had ever pasted the line to find out.

Every wheel is built, installed and tested on four platforms in CI
already. That is not the same as the package resolving from PyPI, and the
README made the stronger claim while `pip install ferrodoc` 404'd.

## What the first real release taught, 2026-08-23

Everything below happened on the way to 0.2.0 and is now either fixed in
the workflows or written into the steps above.

- **crates.io refuses the sixth brand-new crate.** Burst of five, then
  one per ten minutes. Resuming is two named publishes, not a re-run.
- **The wheel could not be built until the facade was on crates.io** —
  and building it for the first time is what found two bugs that had been
  unreachable for two versions: `is_text` was still "everything but
  `docx`", so the Python binding **could not produce an ODT or an EPUB at
  all**, and `formats()` was the union of two directions while the test
  iterated it in one. Both shipped nowhere only because the wheel had
  never built.
- **There is no Intel macOS runner left.** `macos-13` queues forever, and
  because the PyPI publish `needs` every leg, one unschedulable job holds
  the release. That wheel is cross-compiled now, and it is the one wheel
  built without being installed and tested — stated in the workflow
  rather than hidden.
- **`npm publish` is skipped when the version is already there**, so
  re-cutting a release because a *different* artefact failed does not
  fail on "you cannot publish over the previously published versions".

**Re-cutting the same version** is what you do when one artefact failed
and the others are already out. It is safe exactly when the commits since
the tag do not touch what has already been published — check, do not
assume:

```sh
git diff --stat <tag>..HEAD -- crates bindings/wasm bindings/c   # must be empty
gh release delete v0.2.0 --yes
git tag -f v0.2.0 -m 'ferrodoc 0.2.0' && git push --force origin v0.2.0
awk '/^## 0\.2\.0/{f=1;next} /^## /{f=0} f' CHANGELOG.md > /tmp/notes.md
gh release create v0.2.0 --title 'ferrodoc 0.2.0' --notes-file /tmp/notes.md
```

## Known state, 2026-08-23 04:20 GMT — **0.2.0 is out on all three**

- **crates.io** — twelve crates at 0.2.0.
- **npm** — `ferrodoc@0.2.0`, with provenance.
- **PyPI** — 0.2.0: four wheels and an sdist, published by trusted
  publishing on the re-cut release.

Each was checked by **installing it**, not by reading a workflow log:

```console
$ /tmp/v/bin/python -c "import ferrodoc; print(ferrodoc.__version__)"
0.2.0
$ node --input-type=module -e "import {convert} from 'ferrodoc'; console.log(await convert('# a','gfm','html'))"
<h1 id="a">a</h1>
```

That is also how the smoke-test line in this file was found to be wrong —
it said `import('ferrodoc').then(m => m.default())`, which throws.

## Superseded state, 2026-08-23 03:45 GMT

- **crates.io: all twelve at 0.2.0.** The facade's verification build
  compiled against the *published* copies of its eleven dependencies,
  which is what makes the set self-consistent.
- **npm: `ferrodoc@0.2.0` published**, with provenance.
- **PyPI: nothing.** The wheels now build, install and test on Linux,
  Windows and Apple silicon and cross-build for Intel macOS — the whole
  matrix in under three minutes — but they reached that state *after* the
  tag, so the release has to be re-cut for them to publish.
- The tokens pasted into a chat transcript on 2026-08-20 must be
  **rotated**. They are in that transcript.

## Superseded state, 2026-08-23 01:50 GMT

The first run of step 2 stopped where this file now says it would; kept
because it is the evidence behind the rate-limit note above:

- **crates.io: 10 of 12 at 0.2.0.** `ferrodoc-ast`, `-markdown`, `-html`,
  `-text`, `-docx`, `-odt`, `-latex`, `-rst`, `-asciidoc`, `-ipynb`.
- **`ferrodoc-epub` is not published** — it was the *sixth* brand-new
  crate and the burst is five, so crates.io returned 429.
- **`ferrodoc` is not published**, and was never attempted: the workspace
  publish stopped at the refusal. It is at 0.1.0. Being an existing
  crate it is in the 30-per-burst bucket, so it needs no wait of its own
  once `ferrodoc-epub` is up.
- The retry window named in the 429 was **01:42:03 GMT** and has passed.
- **PyPI and npm have nothing.** Both 404. Whatever the confirmation
  emails were, no package exists on either — a pending publisher and an
  `NPM_TOKEN` are configuration, and neither publishes anything until a
  release fires the workflows.
- The two tokens pasted into a chat transcript on 2026-08-20 must be
  **rotated before use**. They are in that transcript.
