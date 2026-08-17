#!/usr/bin/env bash
# Build the C library, compile the example against it, and run it.
#
# The example is not a demo: **a header nobody has called through is a
# guess, not an interface.** Compiling it is what proves the header and
# the library agree, and running it is what proves the ownership rules in
# the header are the ones the library actually follows.
#
# Set VALGRIND=1 to run it under valgrind, which is what CI does — a leak
# of one result per document is invisible in a single conversion and fatal
# in a batch, and it would be a leak in every language binding through
# this ABI.
set -euo pipefail
cd "$(dirname "$0")"

cargo build --release
cc -std=c11 -Wall -Wextra -Werror -I include example/convert.c \
    -L target/release -lferrodoc -o target/convert

if [ "${VALGRIND-}" = 1 ]; then
    LD_LIBRARY_PATH=target/release valgrind \
        --error-exitcode=1 --leak-check=full --errors-for-leak-kinds=definite \
        target/convert
else
    LD_LIBRARY_PATH=target/release target/convert
fi
