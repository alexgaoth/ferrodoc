# ferrodoc-c

The C ABI: one `extern "C"` surface so Go, Java, C#, Ruby and Julia can
link ferrodoc rather than spawn it. Not a workspace member.

## Commands

- `./build.sh` builds the library, compiles `example/convert.c` against
  the header with `-Wall -Wextra -Werror`, and runs it. `VALGRIND=1
  ./build.sh` runs it under valgrind, which is what CI does.
- `scripts/verify.sh --c` from the repository root runs all of it.

## Rules

- **A header nobody has called through is a guess, not an interface.**
  `example/convert.c` is not a demo — compiling it is what proves the
  header and the library agree, and running it is what proves the
  ownership rules the header states are the ones the library follows.
  Every new entry point gets a line in it.
- **Every `unsafe` block is one dereference wide**, immediately preceded
  by the invariant the caller must have met. A block spanning logic is a
  block nobody audits, and `every_unsafe_block_is_one_line_wide` fails
  the build if one appears — it caught a four-line block in the test
  helper first.
- **No panic may cross the boundary.** Unwinding into C is undefined
  behaviour, not a crash you can debug, so `ferrodoc_convert` catches and
  returns the panic as a failed conversion. A long-running host must
  survive a bug in ferrodoc.
- **Every accessor survives NULL.** A C caller will pass one eventually;
  answering is cheaper than a segfault in somebody else's process.
- **The result's bytes are not NUL-terminated.** A `.docx` contains zero
  bytes, so a caller treating the result as a C string truncates it — the
  example checks for a zero byte for exactly that reason.
- `unsafe_code = "allow"` here and **nowhere else**: the workspace forbids
  it, `bindings/wasm` allows only the attribute, and this crate is the one
  place with real blocks. Keep it that way.
- The crate is excluded from the workspace, so `cargo test --workspace`
  does not reach it — run `scripts/verify.sh --c`.
