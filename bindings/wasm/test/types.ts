// The declaration file has to describe the module a caller actually gets.
// A `.d.ts` nobody compiles against is a guess.
import { convert, init, ConversionError, FORMATS } from "../js/ferrodoc.mjs";

async function main(): Promise<void> {
  const html: string | Uint8Array = await convert("# t\n", "markdown", "html");
  const docx: string | Uint8Array = await convert(new Uint8Array([1]), "docx", "gfm");
  await init();
  const names: readonly string[] = FORMATS;
  try {
    await convert("x", "markdown", "nope");
  } catch (error) {
    if (error instanceof ConversionError) {
      const name: "ConversionError" = error.name;
      console.log(name, error.message);
    }
  }
  console.log(html, docx, names.length);
}
void main();
