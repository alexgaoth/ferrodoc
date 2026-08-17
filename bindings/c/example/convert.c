/* A worked example, and the ABI's real test: a header nobody has called
 * through is a guess, not an interface.
 *
 *   cc -I../include convert.c -L../target/release -lferrodoc -o convert
 *   ./convert
 *
 * Every allocation is freed, because CI runs this under valgrind with
 * --error-exitcode=1 — a leak here is a leak in every language that
 * binds through this ABI.
 */
#include <stdio.h>
#include <string.h>
#include "ferrodoc.h"

static int failures = 0;

static void check(const char *what, int passed, const char *detail) {
    printf("%-46s %s%s%s\n", what, passed ? "ok" : "FAILED",
           detail ? " — " : "", detail ? detail : "");
    if (!passed) failures++;
}

int main(void) {
    printf("ferrodoc %s\nformats: %s\n\n", ferrodoc_version(), ferrodoc_formats());

    /* A conversion, and the bytes it produced. */
    const char *markdown = "# Title\n\nHello *world*.\n";
    FerrodocResult *html = ferrodoc_convert((const uint8_t *)markdown,
                                            strlen(markdown), "markdown", "html");
    check("markdown to html succeeds", ferrodoc_result_ok(html), NULL);
    {
        size_t len = ferrodoc_result_len(html);
        const uint8_t *data = ferrodoc_result_data(html);
        check("the html is what it should be",
              len > 0 && memcmp(data, "<h1>Title</h1>", 14) == 0, NULL);
    }
    ferrodoc_result_free(html);

    /* A binary format: the result contains zero bytes, so a caller that
     * treated it as a C string would truncate it at the first one. */
    FerrodocResult *docx = ferrodoc_convert((const uint8_t *)markdown,
                                            strlen(markdown), "markdown", "docx");
    check("markdown to docx succeeds", ferrodoc_result_ok(docx), NULL);
    {
        const uint8_t *data = ferrodoc_result_data(docx);
        size_t len = ferrodoc_result_len(docx);
        check("the docx is a zip archive",
              len > 4 && data[0] == 'P' && data[1] == 'K', NULL);
        check("the docx contains a zero byte, as a C string could not",
              memchr(data, 0, len) != NULL, NULL);
    }
    ferrodoc_result_free(docx);

    /* A failure is a result, not a crash — and the library still works
     * afterwards, which is the property a long-running host needs. */
    const uint8_t garbage[] = {1, 2, 3};
    FerrodocResult *bad = ferrodoc_convert(garbage, sizeof garbage, "docx", "gfm");
    check("bad input is a message, not a crash", !ferrodoc_result_ok(bad), NULL);
    ferrodoc_result_free(bad);

    FerrodocResult *unknown = ferrodoc_convert((const uint8_t *)"x", 1, "markdown", "pdf");
    check("an unknown format is refused", !ferrodoc_result_ok(unknown), NULL);
    ferrodoc_result_free(unknown);

    FerrodocResult *again = ferrodoc_convert((const uint8_t *)"hi", 2, "markdown", "html");
    check("the library still converts after a failure", ferrodoc_result_ok(again), NULL);
    ferrodoc_result_free(again);

    /* An empty document, with a NULL pointer, which is what a caller with
     * an empty buffer will actually pass. */
    FerrodocResult *empty = ferrodoc_convert(NULL, 0, "markdown", "html");
    check("an empty input is an empty document", ferrodoc_result_ok(empty), NULL);
    ferrodoc_result_free(empty);

    /* Freeing NULL is a no-op, as it is for free(). */
    ferrodoc_result_free(NULL);
    check("freeing null does nothing", 1, NULL);

    /* Convert in a loop: a leak of one result per document is invisible
     * in a single conversion and fatal in a batch. */
    for (int i = 0; i < 500; i++) {
        FerrodocResult *r = ferrodoc_convert((const uint8_t *)markdown,
                                             strlen(markdown), "markdown", "gfm");
        if (!ferrodoc_result_ok(r)) { ferrodoc_result_free(r); check("loop", 0, NULL); break; }
        ferrodoc_result_free(r);
    }
    check("500 conversions in a loop", 1, NULL);

    printf("\n%s\n", failures ? "FAILURES" : "all checks passed");
    return failures ? 1 : 0;
}
