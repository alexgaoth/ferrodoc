# ferrodoc-wasm

The `ferrodoc` package on npm: a hand-written WebAssembly binding, plus
the JavaScript that drives it. Not a workspace member.

## Commands

- `./build.sh` compiles the module and copies it to `js/ferrodoc.wasm`,
  which is **git-ignored** — a 1.6 MB binary in git costs every clone
  forever and drifts the moment someone forgets to rebuild. The script
  also fails if the gzipped module passes the 3 MB the README promises.
- `cargo test` (Rust side), `node --test test/ferrodoc.test.mjs` (Node
  side), `node test/browser/run.mjs` (headless Chrome),
  `npx tsc --noEmit` (the declaration file). `scripts/verify.sh --wasm`
  from the repository root runs all four.

## Rules

- **There is no `unsafe` block here, and the test
  `no_unsafe_blocks_in_this_crate` keeps it that way.** The crate sets
  `unsafe_code = "allow"` only because `#[unsafe(no_mangle)]` is an
  attribute the workspace's `forbid` would reject. What makes that
  possible is the **handle table**: buffers stay owned by Rust and
  JavaScript is told a handle plus an address, so nothing here rebuilds a
  slice from a raw pointer. Do not "simplify" it back to
  `slice::from_raw_parts`.
- **No `wasm-bindgen`.** The API is bytes in and bytes out; generated glue
  would be larger than the glue it replaces, and the module's size is a
  published claim.
- **A failure must be a returned handle, never a trap.** A wasm instance
  that panics is poisoned and every later call against it fails, so one
  corrupt document would take the page's converter down. `ferrodoc_ok`
  distinguishes a result from an error message, and two tests — one Rust,
  one browser — check the module still converts after a failure.
- **Rebuild the `Uint8Array` view on every use.** Growing the module's
  memory detaches every existing view of it, and a conversion is exactly
  what grows it. Cache the view and a large document silently reads zeros.
- **Copy results out; never hand back a view.** The slice would dangle the
  moment the next conversion grew the memory.
- Every handle is freed in a `finally`, including on the error path. A
  page converting in a loop otherwise grows its wasm memory until the tab
  dies; `converting in a loop does not leak the module's memory` holds it.
- **Choose the loader by runtime, not by URL scheme.** A browser showing a
  `file://` page still has `fetch` and has no `node:fs` — picking by
  scheme sent it down the Node path, and only the headless-Chrome test
  caught it.
- The declaration file is `ferrodoc.d.mts`, not `.d.ts`: TypeScript
  resolves `./x.mjs` to `./x.d.mts`, and with the wrong extension it
  silently treats the module as `any` while `tsc` still exits 0.
- The browser test talks to Chrome over the DevTools protocol rather than
  through Puppeteer: this package has no dependencies and a test harness
  is a poor reason to acquire one. It also asserts **no network request**,
  which is the claim the package exists for.
