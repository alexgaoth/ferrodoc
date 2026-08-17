# ferrodoc

Convert documents — markdown (CommonMark and GFM), HTML, DOCX and ODT — in
a browser tab or in Node, with **no document leaving the client**.

```sh
npm install ferrodoc
```

```js
import { convert } from "ferrodoc";

const html = await convert("# Title\n\nHello.\n", "markdown", "html");
const docx = await convert(html, "html", "docx");   // Uint8Array
const back = await convert(docx, "docx", "gfm");    // string
```

That is the whole API. Everything the CLI can do is a pair of format
names, and the pandoc AST is reachable through the `json` format rather
than through a second API that would have to be kept in step with it.

## Why this exists

Pandoc is a 153 MB binary you spawn. There is no spawning in a browser
tab, so converting a user's document has meant uploading it. This package
converts it in the tab: **0.6 MB gzipped**, no network request, no server
that could keep a copy.

## What comes back

A `string` for a text format and a `Uint8Array` for `docx` and `odt` —
the same rule the Python binding follows, because a caller writing
markdown to a file wants text and one writing a `.docx` wants bytes.

Formats in: `markdown` (`commonmark`, `md`), `gfm` (`markdown_github`),
`html` (`htm`), `docx`, `odt`, `json`. Those plus `plain` as output.

## Errors

A document that is not the format it claims throws `ConversionError`, and
**the module keeps working afterwards**:

```js
import { convert, ConversionError } from "ferrodoc";

try {
  await convert(bytes, "docx", "gfm");
} catch (error) {
  if (error instanceof ConversionError) console.warn("skipping:", error.message);
}
```

That is worth stating because it is the failure mode a hand-written wasm
binding usually has: a module that traps is poisoned, and every later call
against it fails too, so one bad file would take the page's converter
down with it. There is a test for exactly this.

## Loading

The `.wasm` is fetched relative to the module on first `convert`. To
control when that cost is paid, or to supply the bytes yourself:

```js
import { init } from "ferrodoc";
await init();                      // fetch it now
await init(myArrayBuffer);         // or hand it over
```

## Fidelity

Every conversion is checked against pandoc 3.8.2.1 document by document —
19 differential gates, and two corpora per office format, one of them
written by LibreOffice rather than by us. The numbers, and every known
loss, are in
[`COMPATIBILITY.md`](https://github.com/alexgaoth/ferrodoc/blob/main/COMPATIBILITY.md).

MIT OR Apache-2.0
