// The JavaScript half of ferrodoc's WebAssembly binding.
//
// The surface is deliberately one function, matching the Python binding:
// everything the CLI can do is a pair of format names, and the pandoc AST
// is reachable through the `json` format rather than through a second API
// that would have to be kept in step with it.
//
// No bindgen generated any of this. The wasm module owns every buffer and
// hands out handles; this file writes into linear memory through a fresh
// `Uint8Array` view each time, because the view is detached whenever the
// module's memory grows.

/** A document could not be read as the format it was said to be, or could
 *  not be written as the format asked for. */
export class ConversionError extends Error {
  constructor(message) {
    super(message);
    this.name = "ConversionError";
  }
}

const encoder = new TextEncoder();
const decoder = new TextDecoder();

let wasm = null;

/** Load the module. Called for you on first use; call it yourself only to
 *  control *when* the cost is paid, or to supply the bytesyourself. */
export async function init(source) {
  if (wasm) return wasm;
  let bytes = source;
  if (!bytes) {
    // Resolved relative to this module so a bundler can see it and a
    // plain <script type="module"> works without configuration.
    const url = new URL("./ferrodoc.wasm", import.meta.url);
    // Which runtime, not which URL scheme: a browser showing a file://
    // page still has fetch and has no node:fs, and choosing by scheme
    // sent it down the Node path and failed there.
    const isNode =
      typeof process !== "undefined" && process.versions != null && process.versions.node != null;
    if (isNode) {
      const { readFile } = await import("node:fs/promises");
      bytes = await readFile(url);
    } else {
      bytes = await (await fetch(url)).arrayBuffer();
    }
  }
  // `instantiate` answers with an Instance when handed a Module and with
  // `{ module, instance }` when handed bytes. Both spellings arrive here.
  const module =
    bytes instanceof WebAssembly.Module ? bytes : await WebAssembly.compile(bytes);
  const result = await WebAssembly.instantiate(module, {});
  wasm = (result.instance ?? result).exports;
  return wasm;
}

// A view has to be rebuilt on every use: growing the module's memory
// detaches every existing ArrayBuffer view of it, and a conversion is
// exactly the thing that grows it.
function view(w) {
  return new Uint8Array(w.memory.buffer);
}

function put(w, bytes) {
  const handle = w.ferrodoc_alloc(bytes.length);
  view(w).set(bytes, w.ferrodoc_address(handle));
  return handle;
}

function take(w, handle) {
  const address = w.ferrodoc_address(handle);
  const length = w.ferrodoc_length(handle);
  // Copied out, not referenced: the slice would dangle the moment the
  // next conversion grew the memory.
  return view(w).slice(address, address + length);
}

/**
 * Convert a document from one format to another.
 *
 * Returns a string for a text format and a `Uint8Array` for `docx` and
 * `odt`, which is the same rule the Python binding follows: a caller
 * writing markdown to a file wants text, and a caller writing a `.docx`
 * wants bytes.
 */
export async function convert(data, fromFormat, toFormat) {
  const w = await init();
  const input = typeof data === "string" ? encoder.encode(data) : new Uint8Array(data);
  const handles = [];
  try {
    const inputHandle = put(w, input);
    const from = put(w, encoder.encode(fromFormat));
    const to = put(w, encoder.encode(toFormat));
    handles.push(inputHandle, from, to);
    const out = w.ferrodoc_convert(inputHandle, from, to);
    handles.push(out);
    const bytes = take(w, out);
    if (!w.ferrodoc_ok(out)) throw new ConversionError(decoder.decode(bytes));
    return w.ferrodoc_is_text(to) ? decoder.decode(bytes) : bytes;
  } finally {
    // Freed even when the conversion threw: a page that converts in a
    // loop would otherwise grow its wasm memory until the tab died.
    for (const handle of handles) w.ferrodoc_free(handle);
  }
}

/** Every format name `convert` accepts. */
export const FORMATS = [
  "markdown", "commonmark", "md",
  "gfm", "markdown_github",
  "html", "htm",
  "docx", "odt", "json",
  "plain", "text", "txt",
];
