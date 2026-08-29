#!/usr/bin/env python3
"""Ask pandoc and this binary the same question about one AST.

Every rule in this repository was derived by building a small Pandoc AST,
handing it to both binaries and reading the bytes back. That was fifteen
lines of inline JSON each time, and the boilerplate was where the
mistakes lived: a stray quote, a `Row` written as a tagged object when
the schema wants an array, a case that measured the batch rather than the
construct. This is that fifteen lines, once.

    scripts/probe.sh 'para(code("a`b"))'
    scripts/probe.sh -t rst,latex 'para(sup(words("a b")))'
    scripts/probe.sh -t markdown --columns 30 --wrap auto \\
        'table([0.5, 0.5], [["h1", "h2"]], [["a", "b"]])'

The argument is a Python expression evaluating to one block or a list of
them, with the constructors below in scope. Anything it prints is the
answer *for that one construct*, run one case per invocation — which is
the protocol the batched `ast-sweep.sh` says to confirm findings with.
"""
import argparse, json, subprocess, sys

A = ["", [], []]


def attr(id="", classes=(), kv=()):
    """`{#id .class key=value}`."""
    return [id, list(classes), [list(p) for p in kv]]


def str_(text):
    return {"t": "Str", "c": text}


space = {"t": "Space"}
softbreak = {"t": "SoftBreak"}
linebreak = {"t": "LineBreak"}


def words(text):
    """`"a b"` as `Str "a", Space, Str "b"` — what a reader would build."""
    out = []
    for index, word in enumerate(str(text).split(" ")):
        if index:
            out.append(space)
        out.append(str_(word))
    return out


def _inlines(items):
    """Accept a string, one inline, or a list of either."""
    if isinstance(items, str):
        return words(items)
    if isinstance(items, dict):
        return [items]
    out = []
    for item in items:
        out.extend(_inlines(item))
    return out


def _blocks(items):
    if isinstance(items, dict):
        return [items]
    return [b for item in items for b in _blocks(item)]


def _wrap(tag):
    return lambda inner: {"t": tag, "c": _inlines(inner)}


emph, strong, strikeout = _wrap("Emph"), _wrap("Strong"), _wrap("Strikeout")
sup, sub = _wrap("Superscript"), _wrap("Subscript")
smallcaps, underline = _wrap("SmallCaps"), _wrap("Underline")


def quoted(inner, double=True):
    kind = "DoubleQuote" if double else "SingleQuote"
    return {"t": "Quoted", "c": [{"t": kind}, _inlines(inner)]}


def code(text, a=None):
    return {"t": "Code", "c": [a or A, text]}


def math(text, display=False):
    kind = "DisplayMath" if display else "InlineMath"
    return {"t": "Math", "c": [{"t": kind}, text]}


def raw_inline(fmt, text):
    return {"t": "RawInline", "c": [fmt, text]}


def link(inner, url, title="", a=None):
    return {"t": "Link", "c": [a or A, _inlines(inner), [url, title]]}


def image(inner, url, title="", a=None):
    return {"t": "Image", "c": [a or A, _inlines(inner), [url, title]]}


def span(inner, a=None):
    return {"t": "Span", "c": [a or A, _inlines(inner)]}


def note(blocks):
    return {"t": "Note", "c": _blocks(blocks)}


def cite(keys, inner):
    citations = [
        {"citationId": k, "citationPrefix": [], "citationSuffix": [],
         "citationMode": {"t": "NormalCitation"}, "citationNoteNum": 1,
         "citationHash": 0}
        for k in ([keys] if isinstance(keys, str) else keys)
    ]
    return {"t": "Cite", "c": [citations, _inlines(inner)]}


def para(inner):
    return {"t": "Para", "c": _inlines(inner)}


def plain(inner):
    return {"t": "Plain", "c": _inlines(inner)}


def header(level, inner, a=None):
    return {"t": "Header", "c": [level, a or A, _inlines(inner)]}


def codeblock(text, classes=(), id=""):
    return {"t": "CodeBlock", "c": [attr(id, classes), text]}


def raw_block(fmt, text):
    return {"t": "RawBlock", "c": [fmt, text]}


def quote(blocks):
    return {"t": "BlockQuote", "c": _blocks(blocks)}


def div(blocks, a=None):
    return {"t": "Div", "c": [a or A, _blocks(blocks)]}


def bullets(items):
    return {"t": "BulletList", "c": [_blocks(i) for i in items]}


def ordered(items, start=1, style="Decimal", delim="Period"):
    return {"t": "OrderedList",
            "c": [[start, {"t": style}, {"t": delim}], [_blocks(i) for i in items]]}


def definitions(entries):
    return {"t": "DefinitionList",
            "c": [[_inlines(t), [_blocks(d) for d in ds]] for t, ds in entries]}


def lineblock(lines):
    return {"t": "LineBlock", "c": [_inlines(l) for l in lines]}


rule = {"t": "HorizontalRule"}


def figure(blocks, caption=None, a=None):
    cap = [None, _blocks(caption) if caption else []]
    return {"t": "Figure", "c": [a or A, cap, _blocks(blocks)]}


def table(widths, head, body, aligns=None, caption=None):
    """`widths` is a list of fractions, or `None` for each default."""
    count = len(head[0]) if head else len(body[0])
    aligns = aligns or ["AlignDefault"] * count
    if widths is None:
        widths = [None] * count
    spec = [[{"t": a},
             {"t": "ColWidthDefault"} if w is None else {"t": "ColWidth", "c": w}]
            for a, w in zip(aligns, widths)]

    def cell(text):
        return [A, {"t": "AlignDefault"}, 1, 1, [plain(text)] if text else []]

    def row(cells):
        return [A, [cell(c) for c in cells]]

    cap = [None, _blocks(caption) if caption else []]
    return {"t": "Table",
            "c": [A, cap, spec, [A, [row(r) for r in head]],
                  [[A, 0, [], [row(r) for r in body]]], [A, []]]}


def main():
    ap = argparse.ArgumentParser(add_help=False)
    ap.add_argument("-t", "--to", default="markdown")
    ap.add_argument("-f", "--from", dest="from_", default=None,
                    help="read the expression's output back with this reader")
    ap.add_argument("--columns", default=None)
    ap.add_argument("--wrap", default=None)
    ap.add_argument("--ferrodoc", default="./target/release/ferrodoc")
    ap.add_argument("--json", action="store_true", help="print the AST and stop")
    ap.add_argument("expr")
    args = ap.parse_args()

    blocks = _blocks(eval(args.expr, globals()))  # noqa: S307 - a probe, by hand
    doc = json.dumps({"pandoc-api-version": [1, 23, 1], "meta": {}, "blocks": blocks})
    if args.json:
        print(json.dumps(json.loads(doc), indent=2))
        return 0

    flags = []
    if args.columns:
        flags += ["--columns", args.columns]
    if args.wrap:
        flags += ["--wrap", args.wrap]

    differed = False
    for writer in args.to.split(","):
        def run(cmd):
            r = subprocess.run(cmd, input=doc, capture_output=True, text=True)
            return r.stdout, r.stderr.strip()
        theirs, their_err = run(["pandoc", "-f", "json", "-t", writer] + flags)
        ours, our_err = run([args.ferrodoc, "-f", "json", "-t", writer] + flags)
        same = theirs == ours
        differed |= not same
        print(f"=== {writer}: {'MATCH' if same else 'DIFFER'}")
        if same:
            print("  " + theirs.rstrip("\n").replace("\n", "\n  "))
        else:
            print("  pandoc: " + repr(theirs))
            print("  ours:   " + repr(ours))
        if their_err or our_err:
            print(f"  stderr pandoc: {their_err!r}")
            print(f"  stderr ours:   {our_err!r}")
        # **What the writers produce is only half a rule.** Reading it
        # back says whether matching pandoc's bytes would cost the
        # document — which is how four divergences here were decided.
        if args.from_:
            for name, text in (("pandoc", theirs), ("ours", ours)):
                back = subprocess.run(
                    ["pandoc", "-f", args.from_, "-t", "json"],
                    input=text, capture_output=True, text=True).stdout
                try:
                    got = [b["t"] for b in json.loads(back)["blocks"]]
                except (ValueError, KeyError):
                    got = ["<unreadable>"]
                print(f"  {name} reads back as: {got}")
    return 1 if differed else 0


if __name__ == "__main__":
    sys.exit(main())
