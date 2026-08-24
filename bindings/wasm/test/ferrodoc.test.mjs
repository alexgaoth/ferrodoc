// Tests for the ferrodoc WebAssembly binding.
//
// These check the *binding*, not the converter — the converter is checked
// document by document against pandoc by the Rust harness. What can only
// go wrong here is the boundary: which JavaScript type comes back, what
// happens to bytes that are not the format they claim to be, and whether
// a failure leaves the module usable.
//
//   node --test test/

import { test } from "node:test";
import assert from "node:assert/strict";
import { convert, ConversionError, FORMATS } from "../js/ferrodoc.mjs";

const MARKDOWN = "# Title\n\nHello *world*.\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";

test("text output is a string and binary output is bytes", async () => {
  // The whole reason `convert` inspects the target format: a caller
  // writing markdown to a file wants text, and one writing a .docx wants
  // bytes. Returning bytes for both would make every text caller decode.
  assert.equal(typeof (await convert(MARKDOWN, "gfm", "html")), "string");
  assert.ok((await convert(MARKDOWN, "gfm", "docx")) instanceof Uint8Array);
  assert.ok((await convert(MARKDOWN, "gfm", "odt")) instanceof Uint8Array);
});

test("string and byte input agree", async () => {
  const fromText = await convert(MARKDOWN, "gfm", "html");
  const fromBytes = await convert(new TextEncoder().encode(MARKDOWN), "gfm", "html");
  assert.equal(fromText, fromBytes);
});

test("a table survives markdown to docx and back", async () => {
  // The path the binding exists for, and the structure most easily lost.
  const docx = await convert(MARKDOWN, "gfm", "docx");
  const back = await convert(docx, "docx", "gfm");
  // Every run of whitespace to one space: a pipe table's cells are
  // padded to their column, so `| a | b |` comes back `| a   | b   |`.
  assert.match(back.replace(/[^\S\n]+/g, " "), /\| a \| b \|/);
  assert.match(back, /Title/);
});

test("the AST is reachable through the json format", async () => {
  const ast = JSON.parse(await convert(MARKDOWN, "gfm", "json"));
  assert.deepEqual(ast.blocks.map((b) => b.t), ["Header", "Para", "Table"]);
});

test("an unknown format names the ones that exist", async () => {
  await assert.rejects(() => convert(MARKDOWN, "markdown", "pdf"), (error) => {
    assert.ok(error instanceof ConversionError);
    assert.match(error.message, /pdf/);
    assert.match(error.message, /docx/, "the message should list what is available");
    return true;
  });
});

test("bad input rejects, and the module still works afterwards", async () => {
  // A wasm instance that traps is poisoned: every later call against it
  // fails too. So the binding must turn a bad document into a thrown
  // error while leaving the instance usable — that is what is checked.
  await assert.rejects(
    () => convert(new Uint8Array([1, 2, 3]), "docx", "gfm"),
    ConversionError,
  );
  assert.equal(await convert("hi", "markdown", "html"), "<p>hi</p>\n");
});

test("empty input is an empty document, not an error", async () => {
  // A newline, which is what `pandoc -f commonmark -t html` writes too.
  assert.equal(await convert("", "markdown", "html"), "\n");
});

test("every advertised format is one convert accepts", async () => {
  for (const name of FORMATS) {
    await convert("x", "markdown", name);
  }
});

test("converting in a loop does not leak the module's memory", async () => {
  // Every handle is freed in a `finally`, including on the error path.
  // Without that a page converting in a loop grows its wasm memory until
  // the tab dies — the failure this binding is most likely to have.
  const { init } = await import("../js/ferrodoc.mjs");
  const wasm = await init();
  const big = MARKDOWN.repeat(200);
  await convert(big, "gfm", "docx");
  const before = wasm.memory.buffer.byteLength;
  for (let i = 0; i < 50; i++) await convert(big, "gfm", "docx");
  assert.equal(wasm.memory.buffer.byteLength, before,
    "linear memory grew across repeated conversions");
});
