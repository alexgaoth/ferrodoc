/* ferrodoc — convert documents in your own process.
 *
 * One entry point: bytes and two format names in, a result out. Link
 * against libferrodoc (a cdylib or a staticlib; `cargo build --release`
 * produces both).
 *
 * Ownership: every result belongs to the library until you free it, and
 * the bytes it points at are valid for exactly that long. Copy them if
 * you need them afterwards.
 */
#ifndef FERRODOC_H
#define FERRODOC_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* The outcome of a conversion. Opaque; use the accessors below. */
typedef struct FerrodocResult FerrodocResult;

/* Convert `len` bytes at `data` from one named format to another.
 *
 * Never returns NULL. The result holds either the converted document or
 * a message saying why not — ask ferrodoc_result_ok(). Release it with
 * ferrodoc_result_free().
 *
 * `data` may be NULL when `len` is 0. A conversion that fails is a
 * result, not a crash, and neither is a bug inside ferrodoc: nothing
 * unwinds across this boundary. */
FerrodocResult *ferrodoc_convert(const uint8_t *data, size_t len,
                                 const char *from, const char *to);

/* 1 if the result is a converted document, 0 if it is a message. */
int ferrodoc_result_ok(const FerrodocResult *result);

/* The result's bytes, valid until it is freed. Not NUL-terminated: a
 * .docx contains zero bytes, so use ferrodoc_result_len(). */
const uint8_t *ferrodoc_result_data(const FerrodocResult *result);

/* How many bytes the result holds. */
size_t ferrodoc_result_len(const FerrodocResult *result);

/* Release a result. Freeing NULL does nothing. */
void ferrodoc_result_free(FerrodocResult *result);

/* The library version. Static; never free it. */
const char *ferrodoc_version(void);

/* Every format name, comma-separated. Static; never free it. */
const char *ferrodoc_formats(void);

#ifdef __cplusplus
}
#endif

#endif /* FERRODOC_H */
