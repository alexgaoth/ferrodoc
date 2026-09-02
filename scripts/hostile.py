#!/usr/bin/env python3
"""Every reader against shapes chosen to break it, not to be converted.

**This axis is generated, not collected.** Every other reader gate in
this project scores a corpus somebody wrote down — the CommonMark spec,
twenty documents — and a gate cannot fail on a construct its corpus
lacks. On 2026-09-02 that cost a process abort: `tex_inlines` recursed
without a bound, reachable from every reader that can carry a `Math`
node, and no corpus document contains a braced expression for the
fuzzer to mutate into one.

So the contract here is not fidelity. It is the one in
`ROADMAP.md`'s continuous obligations — **reject hostile input within
documented limits** — reduced to something a script can decide:

    every conversion either writes bytes or returns a clean error.
    Never a signal, never a panic, never a hang.

A refusal is a pass. `document nests containers 200 or more levels deep`
is the bound working. What fails is `fatal runtime error: stack
overflow`, a `panicked at`, a kill, or a timeout.

    ./scripts/hostile.sh            every case, and every failure
    ./scripts/hostile.sh --floors   silent unless something regressed
"""
import sys

# **`scripts/math.py` shadows the standard library's `math`.** Python puts
# this script's own directory first on the path, so every later import
# resolves `math` to the TeX gate beside this file — including
# `selectors`, which `subprocess` reaches for the moment a call has a
# timeout, and which then dies on `math.ceil`. Drop the script directory
# before importing anything that could care.
if sys.path and sys.path[0].endswith("scripts"):
    del sys.path[0]

import json, os, subprocess  # noqa: E402

# **Two depths, because two different things are being outrun.**
#
# `DEEP` is for a *documented bound*: every structural reader refuses at
# 200, so 5_000 is twenty-five times past it and proves the refusal
# without paying for depth nobody checks. Raising it only buys runtime —
# and expensively, since refusing nested HTML is quadratic (see
# COMPATIBILITY.md): the same case at 50_000 cost 59 of this gate's 98
# seconds and asked no question the 5_000 one does not.
#
# `STACK` is for a *stack frame*, where the bound is the hardware's. It
# is sized for the cheapest frame, not the average one: a braced group
# costs two frames per level and overflows at 20_000, while a `\left`
# chain costs one and sailed through that same depth with the bound
# removed. A depth that only catches the expensive path is a gate that
# passes the bug next to the one it was written for.
DEEP = 5_000
STACK = 50_000
LONG = 1_000_000  # one very large token, not a deep one

def md_list(n):
    return "".join("  " * i + "- x\n" for i in range(n))

def ipynb(source):
    return json.dumps({
        "cells": [{"cell_type": "markdown", "metadata": {}, "source": [source]}],
        "metadata": {}, "nbformat": 4, "nbformat_minor": 5,
    })

def pandoc_json(inline):
    return json.dumps({"pandoc-api-version": [1, 23, 1], "meta": {},
                       "blocks": [{"t": "Para", "c": [inline]}]})

def math(tex):
    return {"t": "Math", "c": [{"t": "InlineMath"}, tex]}

# **Both halves matter.** A shape that the reader refuses proves the
# bound; a shape it accepts proves the writers survive what the reader
# let through — which is the half that was missing, because the abort
# was on the writer side of a document every reader accepted.
CASES = [
    # --- nesting, past every documented bound -------------------------
    ("md-blockquote",   "markdown", "> " * DEEP + "hi\n"),
    ("md-list",         "markdown", md_list(DEEP // 10)),
    ("md-emphasis",     "markdown", "*" * DEEP + "x" + "*" * DEEP + "\n"),
    ("md-brackets",     "markdown", "[" * DEEP + "x" + "]" * DEEP + "\n"),
    ("md-html-block",   "markdown", "<div>" * DEEP + "x\n"),
    ("md-code-indent",  "markdown", "".join(" " * (4 * i) + "x\n" for i in range(DEEP // 10))),
    ("gfm-blockquote",  "gfm",      "> " * DEEP + "hi\n"),
    ("gfm-emphasis",    "gfm",      "~" * DEEP + "x" + "~" * DEEP + "\n"),
    ("pmd-blockquote",  "pandoc_markdown", "> " * DEEP + "hi\n"),
    ("html-div",        "html",     "<div>" * DEEP + "x"),
    ("html-span-in-pre","html",     "<pre>" + "<span>" * DEEP + "x"),
    ("html-table",      "html",     "<table><tr><td>" * DEEP + "x"),
    ("html-list",       "html",     "<ul><li>" * DEEP + "x"),
    ("json-emph",       "json",     None),   # built below
    ("ipynb-blockquote","ipynb",    ipynb("> " * DEEP + "hi")),

    # --- math: the shape that got through -----------------------------
    ("md-math-braces",  "markdown", "$" + "{" * STACK + "x" + "}" * STACK + "$\n"),
    ("md-math-left",    "markdown", "$" + r"\left" * STACK + "(" + "$\n"),
    ("md-math-scripts", "markdown", "$x" + "^{x" * STACK + "}" * STACK + "$\n"),
    ("md-math-display", "markdown", "$$" + "{" * STACK + "x" + "}" * STACK + "$$\n"),
    ("md-math-accent",  "markdown", "$" + r"\hat{" * STACK + "x" + "}" * STACK + "$\n"),
    ("json-math-braces","json",     pandoc_json(math("{" * STACK + "x" + "}" * STACK))),
    ("json-math-left",  "json",     pandoc_json(math(r"\left" * STACK + "("))),
    ("ipynb-math",      "ipynb",    ipynb("$" + "{" * STACK + "x" + "}" * STACK + "$")),

    # --- length rather than depth -------------------------------------
    ("md-long-line",    "markdown", "a" * LONG + "\n"),
    ("md-long-word-run","markdown", ("ab " * (LONG // 3)) + "\n"),
    ("md-many-blanks",  "markdown", "\n" * (LONG // 10) + "x\n"),
    ("md-long-code",    "markdown", "```\n" + "x" * LONG + "\n```\n"),
    ("html-long-attr",  "html",     "<div " + "a=1 " * 100_000 + ">x</div>"),
    ("html-long-text",  "html",     "<p>" + "a" * LONG + "</p>"),
    ("md-wide-table",   "markdown",
        "|" + "h|" * 2_000 + "\n|" + "-|" * 2_000 + "\n|" + "c|" * 2_000 + "\n"),

    # --- unbalanced and truncated -------------------------------------
    ("md-unclosed-fence", "markdown", "```\nx\n"),
    ("md-unclosed-math",  "markdown", "$x\n"),
    ("md-unclosed-link",  "markdown", "[a](b\n"),
    ("html-unclosed",     "html",     "<div><span><p>x"),
    ("json-truncated",    "json",     '{"pandoc-api-version":[1,23,1],"meta":{},"blo'),
    ("ipynb-empty",       "ipynb",    "{}"),
]

# Nested Emph, one level per frame, built as text so python's own
# recursion limit is not the thing under test.
_emph = ('{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[{"t":"Para","c":['
         + '{"t":"Emph","c":[' * DEEP + '{"t":"Str","c":"x"}' + ']}' * DEEP
         + ']}]}')
CASES = [(n, f, _emph if n == "json-emph" else t) for n, f, t in CASES]

# The writers that recurse, and the three that render math.
WRITERS = ["html", "plain", "commonmark", "latex", "rst", "asciidoc", "json"]

TIMEOUT = 60


def classify(code, err):
    """A pass writes bytes or says why it will not."""
    if code == 0:
        return None
    if code == 1 and err.startswith("ferrodoc:"):
        return None
    if code == 124:
        return f"hung past {TIMEOUT}s"
    first = (err.strip().splitlines() or [""])[0][:90]
    # **A signal is a negative returncode here, not 128 + n.** `subprocess`
    # reports it that way, and a stack overflow arrives as SIGABRT — the
    # exact shape this axis exists to catch, so getting the sign wrong
    # would have made the gate pass through the bug it was written for.
    if code < 0:
        return f"killed by signal {-code}: {first}"
    if code == 101 or "panicked at" in err:
        return f"panicked: {first}"
    return f"exit {code}: {first}"


def main():
    floors = "--floors" in sys.argv
    binary = "./target/release/ferrodoc"
    tmp = os.environ.get("TMPDIR", "/tmp") + "/hostile-input"
    failures, ran = [], 0

    for name, reader, text in CASES:
        with open(tmp, "w") as fh:
            fh.write(text)
        for writer in WRITERS:
            ran += 1
            try:
                done = subprocess.run(
                    [binary, "-f", reader, "-t", writer, tmp],
                    capture_output=True, timeout=TIMEOUT)
                code, err = done.returncode, done.stderr.decode("utf8", "replace")
            except subprocess.TimeoutExpired:
                code, err = 124, ""
            bad = classify(code, err)
            if bad:
                failures.append((name, reader, writer, bad))
                if not floors:
                    print(f"  {name:<20} {reader:>16} -> {writer:<11} {bad}")

    if os.path.exists(tmp):
        os.remove(tmp)
    if not floors:
        print(f"{ran - len(failures)}/{ran} conversions of "
              f"{len(CASES)} hostile shapes returned rather than died")
    if failures:
        if floors:
            for name, reader, writer, bad in failures:
                print(f"  {name} {reader} -> {writer}: {bad}", file=sys.stderr)
            print(f"{len(failures)} hostile shapes did not return", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
