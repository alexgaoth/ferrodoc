#!/usr/bin/env python3
"""Every TeX expression this renders, against pandoc's own rendering.

**Pandoc does not write the TeX source** for `commonmark`, `html` or
`plain`: it converts the expression to ordinary inlines — a variable is
emphasis, `^` is a superscript, `\\alpha` is `α` — and lets each writer
render those. Where the expression is more than symbols and scripts it
gives up and writes the source between dollars.

`scripts/ast-sweep.sh` asks about `$x^2$` and nothing else, so it cannot
say how much of the language this covers. **This can.** Each expression
is rendered by both binaries in all three writers and compared byte for
byte; the score is the number of renderings that agree, and the floor
below is the contract.

A case that *both* binaries fall back on counts as identical, which is
the honest reading: the fallback is pandoc's own, and writing the source
is what pandoc does with a fraction.

    ./scripts/math.sh              the score, and every disagreement
    ./scripts/math.sh --floors     silent unless the floor is missed
"""
import json, subprocess, sys

# **What pandoc renders, and what it gives up on.** Both belong here: a
# case that falls back on both sides is a case this must keep falling
# back on, and one that renders on both is a rule this must keep.
CASES = [
    # scripts
    "x^2", "x_i", "x^{2n}", "x_{ij}", "A^{T}", "10^{-3}", "H_2O",
    "\\theta_0", "v_x^2", "x_{i=1}", "e^{-x}", "x^{a+b}",
    # variables, digits and brackets
    "f(x)", "ab", "2x", "n!", "(a,b)", "f(x,y)", "a;b", "[x]", "|x|",
    # operators and relations, and the spaces around them
    "a+b", "a-b", "-x", "a \\times b", "a \\cdot b", "a \\div b",
    "E = mc^2", "x \\leq y", "x \\geq y", "x \\neq y", "x \\approx y",
    "x \\in A", "A \\subset B", "a \\to b", "a \\Rightarrow b",
    "x \\pm y", "a \\cup b", "a \\cap b",
    # symbols
    "\\alpha", "\\alpha x", "\\pi r^2", "\\Omega", "\\infty", "\\partial",
    "\\nabla", "\\forall", "\\exists", "\\emptyset", "\\ldots", "\\sum",
    "\\sum x", "\\int", "\\prod", "\\aleph", "\\hbar", "\\ell",
    # fonts and accents
    "\\mathbb{R}", "\\mathcal{L}", "\\text{hi}", "\\mathrm{d}x",
    "\\mathbf{x}", "\\mathit{y}", "\\hat{x}", "\\bar{x}", "\\vec{v}",
    "\\tilde{n}", "\\dot{x}", "\\ddot{x}",
    # brackets and escapes
    "\\left(x\\right)", "\\{x\\}", "\\%", "\\$", "\\#",
    # what pandoc itself will not render
    "\\frac{1}{2}", "\\sqrt{x}", "\\sum_{i=1}^n", "\\sum_{i}^n",
    "x \\\\ y", "\\begin{matrix}a\\end{matrix}", "\\underbrace{x}",
]

WRITERS = ("plain", "html", "commonmark")
FERRODOC = "./target/release/ferrodoc"
# **242 of 243, and the one that differs is a decision already recorded.**
# `$x \\\\ y$` is written verbatim by pandoc and with the backslash
# escaped here, which is the rule COMPATIBILITY.md records as "a backslash
# before ASCII punctuation ... where pandoc writes `\\\\` and drops the
# character after it". The fallback is text in `commonmark`, so it takes
# the escaping text takes.
FLOOR = 242


def render(tex, writer, binary):
    doc = {"pandoc-api-version": [1, 23, 1], "meta": {},
           "blocks": [{"t": "Para", "c": [{"t": "Math", "c": [{"t": "InlineMath"}, tex]}]}]}
    out = subprocess.run([binary, "-f", "json", "-t", writer, "--wrap=none"],
                         input=json.dumps(doc), capture_output=True, text=True)
    return out.stdout.strip()


def main():
    floors_only = "--floors" in sys.argv[1:]
    total = len(CASES) * len(WRITERS)
    differ = []
    for tex in CASES:
        for writer in WRITERS:
            theirs = render(tex, writer, "pandoc")
            ours = render(tex, writer, FERRODOC)
            if theirs != ours:
                differ.append((tex, writer, theirs, ours))
    score = total - len(differ)
    if not floors_only or score < FLOOR:
        for tex, writer, theirs, ours in differ:
            print(f"  {tex!r} ({writer})")
            print(f"      pandoc: {theirs!r}")
            print(f"      ours:   {ours!r}")
    print(f"{score}/{total} renderings of {len(CASES)} expressions identical")
    if score < FLOOR:
        print(f"BELOW ITS FLOOR OF {FLOOR}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
