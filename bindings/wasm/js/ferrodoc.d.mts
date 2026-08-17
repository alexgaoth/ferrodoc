/** A document could not be read as the format it was said to be, or could
 *  not be written as the format asked for. */
export class ConversionError extends Error {
  readonly name: "ConversionError";
}

/** Every format name {@link convert} accepts. */
export const FORMATS: readonly string[];

/**
 * Load the WebAssembly module.
 *
 * Called for you on first {@link convert}; call it yourself only to
 * control *when* the cost is paid, or to supply the bytes (a `Response`
 * body, a bundler's asset import, an already-compiled `Module`).
 */
export function init(source?: BufferSource | WebAssembly.Module): Promise<WebAssembly.Exports>;

/**
 * Convert a document from one format to another.
 *
 * Returns a `string` for a text format and a `Uint8Array` for `docx` and
 * `odt` — the same rule the Python binding follows, because a caller
 * writing markdown to a file wants text and one writing a `.docx` wants
 * bytes.
 *
 * Input formats: `markdown` (`commonmark`, `md`), `gfm`
 * (`markdown_github`), `html` (`htm`), `docx`, `odt`, `json`. Those plus
 * `plain` (`text`, `txt`) as output.
 *
 * The format is typed as `string` rather than a union of literals on
 * purpose: a name usually arrives from a config file, a file extension or
 * a MIME lookup, and a stricter type would make the common call site fail
 * to type-check for no benefit.
 *
 * @throws {ConversionError} if the input is not the format it claims, or
 * the output format does not exist.
 */
export function convert(
  data: string | Uint8Array | ArrayBuffer,
  fromFormat: string,
  toFormat: string,
): Promise<string | Uint8Array>;
