#!/usr/bin/env python3
"""Enumerate the AST and ask both writers about every construct.

A document corpus can only fail on what somebody wrote into it. The AST
cannot hide anything: it is a finite set of variants, so walking it finds
every construct where the two writers disagree, in one pass, named.

Discovery is batched — all cases in one document, split on a sentinel —
because that is 4 process spawns instead of 400. A batch can misalign, so
the sentinel count is checked, and anything it reports must be confirmed
one-case-per-invocation before it is believed.
"""
import json, re, subprocess, sys

A = ["", [], []]
def attr(i="", c=(), kv=()):
    return [i, list(c), [list(p) for p in kv]]

S = lambda t: {"t": "Str", "c": t}
SP = {"t": "Space"}
def inls(*words):
    out = []
    for i, w in enumerate(words):
        if i: out.append(SP)
        out.append(S(w))
    return out

def plain(*w): return {"t": "Plain", "c": inls(*w)}
def para(*w): return {"t": "Para", "c": inls(*w)}
def wrap(tag, *w): return {"t": tag, "c": inls(*w)}
def cell(t):
    return [A, {"t": "AlignDefault"}, 1, 1, [plain(t)] if t else []]
def row(ts): return [A, [cell(t) for t in ts]]

def table(aligns, head, body, caption=None, widths=None):
    spec = [[{"t": a}, ({"t": "ColWidth", "c": widths[n]} if widths else {"t": "ColWidthDefault"})]
            for n, a in enumerate(aligns)]
    cap = [None, ([plain(caption)] if caption else [])]
    return {"t": "Table", "c": [A, cap, spec,
                                [A, [row(h) for h in head]],
                                [[A, 0, [], [row(b) for b in body]]],
                                [A, []]]}

CASES = []
def case(name, block):
    CASES.append((name, block))

# ---- inlines, each alone in a paragraph -----------------------------------
for tag in ["Emph", "Strong", "Strikeout", "Superscript", "Subscript",
            "SmallCaps", "Underline"]:
    case(f"inline/{tag}", {"t": "Para", "c": [wrap(tag, "x")]})
    case(f"inline/{tag}+space", {"t": "Para", "c": [wrap(tag, "a", "b")]})
for kind in ["SingleQuote", "DoubleQuote"]:
    case(f"inline/Quoted.{kind}",
         {"t": "Para", "c": [{"t": "Quoted", "c": [{"t": kind}, inls("x")]}]})
for kind in ["InlineMath", "DisplayMath"]:
    # `x^2` is one pandoc **renders** to markup; `\frac` is one it gives
    # up on and writes back as TeX. Only the first was here, so the
    # fallback — which is what most real math hits — went unmeasured,
    # and both the HTML and plain writers had it wrong.
    case(f"inline/Math.{kind}",
         {"t": "Para", "c": [{"t": "Math", "c": [{"t": kind}, "x^2"]}]})
    case(f"inline/Math.{kind}.unrenderable",
         {"t": "Para", "c": [{"t": "Math", "c": [{"t": kind}, "\\frac{a}{b}"]}]})
case("inline/Code", {"t": "Para", "c": [{"t": "Code", "c": [A, "a b"]}]})
case("inline/Code+attr", {"t": "Para", "c": [{"t": "Code", "c": [attr("i", ["c"]), "x"]}]})
case("inline/Code+backtick", {"t": "Para", "c": [{"t": "Code", "c": [A, "a ` b"]}]})
case("inline/RawInline.html", {"t": "Para", "c": [{"t": "RawInline", "c": ["html", "<b>x</b>"]}]})
case("inline/RawInline.tex", {"t": "Para", "c": [{"t": "RawInline", "c": ["tex", "\\x"]}]})
case("inline/LineBreak", {"t": "Para", "c": [S("a"), {"t": "LineBreak"}, S("b")]})
case("inline/SoftBreak", {"t": "Para", "c": [S("a"), {"t": "SoftBreak"}, S("b")]})
case("inline/Link", {"t": "Para", "c": [{"t": "Link", "c": [A, inls("t"), ["u", ""]]}]})
case("inline/Link+title", {"t": "Para", "c": [{"t": "Link", "c": [A, inls("t"), ["u", "ti"]]}]})
case("inline/Link+attr", {"t": "Para", "c": [{"t": "Link", "c": [attr("i", ["c"]), inls("t"), ["u", ""]]}]})
case("inline/Link.empty-text", {"t": "Para", "c": [{"t": "Link", "c": [A, [], ["u", ""]]}]})
case("inline/Image", {"t": "Para", "c": [S("a"), SP, {"t": "Image", "c": [A, inls("alt"), ["p", ""]]}]})
# **A distinct alt text**, because RST names a substitution after it and
# uniquifies a repeat to `|image1|` — two cases sharing "alt" made the
# second look like a writer difference when both writers agree.
case("inline/Image+title", {"t": "Para", "c": [S("a"), SP, {"t": "Image", "c": [A, inls("titled"), ["p", "ti"]]}]})
case("inline/Span+attr", {"t": "Para", "c": [{"t": "Span", "c": [attr("i", ["c"]), inls("x")]}]})
case("inline/Span.bare", {"t": "Para", "c": [{"t": "Span", "c": [A, inls("x")]}]})
case("inline/Note", {"t": "Para", "c": [S("a"), {"t": "Note", "c": [para("body")]}]})
def _citation(key, mode="NormalCitation", prefix=(), suffix=()):
    return {"citationId": key, "citationPrefix": list(prefix),
            "citationSuffix": list(suffix), "citationMode": {"t": mode},
            "citationNoteNum": 1, "citationHash": 0}


# **Pandoc writes a citation from its data and ignores the rendered
# inlines**, so the mode and the affixes are the whole construct — one
# case with one key could not see any of it.
case("inline/Cite", {"t": "Para", "c": [
    {"t": "Cite", "c": [[_citation("k")], inls("[@k]")]}]})
case("inline/Cite.author-in-text", {"t": "Para", "c": [
    {"t": "Cite", "c": [[_citation("k", "AuthorInText")], inls("@k")]}]})
case("inline/Cite.suppress-author", {"t": "Para", "c": [
    {"t": "Cite", "c": [[_citation("k", "SuppressAuthor")], inls("[-@k]")]}]})
case("inline/Cite.two-keys", {"t": "Para", "c": [
    {"t": "Cite", "c": [[_citation("k"), _citation("k2")], inls("[@k; @k2]")]}]})
case("inline/Cite.affixes", {"t": "Para", "c": [
    {"t": "Cite", "c": [[_citation("k", prefix=inls("see"), suffix=inls("p. 3"))],
                        inls("[see @k p. 3]")]}]})
# text that needs escaping
for ch in ["*", "_", "`", "#", "[", "]", "<", ">", "|", "~", "^", "\\", "'", '"',
           "$", "@", "%", "{", "}", "-", "+", "!", "&"]:
    case(f"escape/{ch}", {"t": "Para", "c": [S(f"a{ch}b")]})
for lead in ["#", "-", "+", "*", ">", "1.", "1)", "2.", "    ", "==="]:
    case(f"escape/line-start {lead!r}", {"t": "Para", "c": [S(lead), SP, S("x")]})
# **The same leads on a *continuation* line, which is a different
# question and was never asked.** Only a block's first line can open a
# block, so an ordered marker further down is ordinary text: pandoc
# writes `128.` bare, and this wrote `128\.` into the middle of a
# paragraph for as long as nothing measured it.
#
# A **hard** break, because this sweep runs `--wrap=none` and a
# `SoftBreak` collapses to a space there — a soft-continuation case would
# be one line of prose wearing a name about line starts. The rule is the
# same for both breaks (probed), and the soft form is what the prose in
# the writers corpus exercises.
for lead in ["#", "-", "+", "*", ">", "1.", "1)", "2.", "    ", "==="]:
    case(f"escape/continuation {lead!r}",
         {"t": "Para", "c": [S("a"), {"t": "LineBreak"}, S(lead), SP, S("x")]})
case("escape/em-dash", {"t": "Para", "c": [S("a—b")]})
# The dialect un-smartens these back to straight quotes, and the corpus
# saw it only through one drop-in row.
for name, ch in [("lsquo", "\u2018"), ("rsquo", "\u2019"),
                 ("ldquo", "\u201c"), ("rdquo", "\u201d")]:
    case(f"escape/{name}", {"t": "Para", "c": [S(f"a{ch}b")]})
case("escape/ellipsis", {"t": "Para", "c": [S("a…b")]})
case("escape/nbsp", {"t": "Para", "c": [S("a b")]})

# ---- blocks ---------------------------------------------------------------
case("block/HorizontalRule", {"t": "HorizontalRule"})
for lvl in [1, 2, 3, 6]:
    case(f"block/Header{lvl}", {"t": "Header", "c": [lvl, A, inls("Title")]})
case("block/Header+id", {"t": "Header", "c": [1, attr("custom"), inls("Title")]})
case("block/Header+class", {"t": "Header", "c": [1, attr("", ["c"]), inls("Title")]})
case("block/Header+kv", {"t": "Header", "c": [1, attr("", [], [("k", "v")]), inls("Title")]})
case("block/LineBlock", {"t": "LineBlock", "c": [inls("one"), inls("two")]})
case("block/BlockQuote", {"t": "BlockQuote", "c": [para("q")]})
case("block/BlockQuote.nested", {"t": "BlockQuote", "c": [{"t": "BlockQuote", "c": [para("q")]}]})
case("block/BulletList", {"t": "BulletList", "c": [[plain("a")], [plain("b")]]})
case("block/BulletList.loose", {"t": "BulletList", "c": [[para("a")], [para("b")]]})
case("block/BulletList.nested", {"t": "BulletList", "c": [[plain("a"), {"t": "BulletList", "c": [[plain("b")]]}]]})
for style in ["Decimal", "LowerAlpha", "UpperAlpha", "LowerRoman", "UpperRoman", "Example", "DefaultStyle"]:
    for delim in ["Period", "OneParen", "TwoParens", "DefaultDelim"]:
        case(f"block/OrderedList.{style}.{delim}",
             {"t": "OrderedList", "c": [[1, {"t": style}, {"t": delim}], [[plain("a")]]]})
case("block/OrderedList.start3",
     {"t": "OrderedList", "c": [[3, {"t": "Decimal"}, {"t": "Period"}], [[plain("a")]]]})
case("block/DefinitionList",
     {"t": "DefinitionList", "c": [[inls("term"), [[para("def")]]]]})
case("block/DefinitionList.two-defs",
     {"t": "DefinitionList", "c": [[inls("term"), [[para("d1")], [para("d2")]]]]})
# A `Plain` definition is a **tight** list, which LaTeX marks
# `\tightlist` and the loose one does not.
case("block/DefinitionList.tight",
     {"t": "DefinitionList", "c": [[inls("term"), [[plain("def")]]]]})
for classes in [[], ["bash"], ["sourceCode", "bash"], ["sourceCode"]]:
    case(f"block/CodeBlock{classes}", {"t": "CodeBlock", "c": [attr("", classes), "x"]})
case("block/CodeBlock+id", {"t": "CodeBlock", "c": [attr("i", ["bash"]), "x"]})
case("block/CodeBlock.newline", {"t": "CodeBlock", "c": [attr("", ["text"]), "x\n"]})
case("block/CodeBlock.empty", {"t": "CodeBlock", "c": [attr("", ["text"]), ""]})
case("block/CodeBlock.backticks", {"t": "CodeBlock", "c": [attr("", ["text"]), "```"]})
case("block/RawBlock.html", {"t": "RawBlock", "c": ["html", "<p>x</p>"]})
case("block/RawBlock.tex", {"t": "RawBlock", "c": ["tex", "\\x"]})
case("block/Div+class", {"t": "Div", "c": [attr("", ["callout"]), [para("body")]]})
case("block/Div.bare", {"t": "Div", "c": [A, [para("body")]]})
case("block/Div+id+classes", {"t": "Div", "c": [attr("i", ["a", "b"]), [para("body")]]})
# pandoc's HTML reader wraps every highlighted code block in one of these
case("block/Div.sourceCode-wrapper",
     {"t": "Div", "c": [attr("cb1", ["sourceCode"]),
                        [{"t": "CodeBlock", "c": [attr("", ["sourceCode", "bash"]), "x"]}]]})
case("block/Figure", {"t": "Figure", "c": [A, [None, [plain("cap")]],
                                           [{"t": "Plain", "c": [{"t": "Image", "c": [A, inls("alt"), ["p", ""]]}]}]]})
case("block/Table.simple", table(["AlignDefault"] * 2, [["h1", "h2"]], [["a", "b"]]))
case("block/Table.aligned", table(["AlignLeft", "AlignCenter", "AlignRight"],
                                  [["L", "C", "R"]], [["a", "b", "c"]]))
case("block/Table.no-header", table(["AlignDefault"] * 2, [["", ""]], [["a", "b"]]))
case("block/Table.caption", table(["AlignDefault"] * 2, [["h1", "h2"]], [["a", "b"]], caption="Cap"))
case("block/Table.widths", table(["AlignDefault"] * 2, [["h1", "h2"]], [["a", "b"]], widths=[0.5, 0.5]))
# A cell too wide for its column: the row becomes several lines under
# `--wrap=auto`, and the column widens instead under `--wrap=preserve`.
case("block/Table.widths.wrapping",
     table(["AlignDefault"] * 2, [["h1", "h2"]],
           [["one two three four five six seven eight nine ten", "b"]],
           widths=[0.5, 0.5]))
case("block/Table.widths.uneven",
     table(["AlignDefault"] * 3, [["h1", "h2", "h3"]], [["a", "b", "c"]],
           widths=[0.25, 0.25, 0.5]))
case("block/Table.empty-cells", table(["AlignDefault"] * 2, [["a", "b"]], [["", ""]]))

# ---- composition: every construct inside every container ------------------
# **Enumeration covers the alphabet; this is the sentences.** Everything
# above asks whether a writer knows a construct *at all*, each one alone
# at the top of a document. It cannot ask whether two layers compose,
# which is where this project's writer bugs have actually lived: a
# backtick that is right in a paragraph and wrong in a table cell, an
# indent that is right at top level and lost inside a list item. Written
# as a cross product rather than picked, for the same reason the axis
# above beat a document corpus — a chosen case is blind wherever its
# author was.

def _table_of(blocks):
    """A one-column table whose single body cell holds `blocks`."""
    spec = [[{"t": "AlignDefault"}, {"t": "ColWidthDefault"}]]
    body_cell = [A, {"t": "AlignDefault"}, 1, 1, blocks]
    return {"t": "Table", "c": [A, [None, []], spec,
                                [A, [row(["h"])]],
                                [[A, 0, [], [[A, [body_cell]]]]],
                                [A, []]]}

INLINE_IN = [
    ("Emph", lambda i: {"t": "Para", "c": [{"t": "Emph", "c": [i]}]}),
    ("Link", lambda i: {"t": "Para", "c": [{"t": "Link", "c": [A, [i], ["u", ""]]}]}),
    ("Header", lambda i: {"t": "Header", "c": [2, A, [i]]}),
    ("Quoted", lambda i: {"t": "Para", "c": [
        {"t": "Quoted", "c": [{"t": "DoubleQuote"}, [i]]}]}),
    ("Note", lambda i: {"t": "Para", "c": [
        S("a"), {"t": "Note", "c": [{"t": "Para", "c": [i]}]}]}),
    ("Term", lambda i: {"t": "DefinitionList", "c": [[[i], [[para("d")]]]]}),
    ("Cell", lambda i: _table_of([{"t": "Plain", "c": [i]}])),
]

INNER_INLINES = [
    ("Code", {"t": "Code", "c": [A, "a`b"]}),
    ("Math", {"t": "Math", "c": [{"t": "InlineMath"}, "x^2"]}),
    ("Link", {"t": "Link", "c": [A, inls("t"), ["u", ""]]}),
    ("Image", {"t": "Image", "c": [A, inls("pic"), ["p", ""]]}),
    ("Note", {"t": "Note", "c": [para("n")]}),
    ("Strikeout", wrap("Strikeout", "s")),
    ("SmallCaps", wrap("SmallCaps", "s")),
    ("LineBreak", {"t": "LineBreak"}),
    ("RawInline", {"t": "RawInline", "c": ["html", "<b>x</b>"]}),
    ("markup-chars", S("*x*")),
]

for _outer, _build in INLINE_IN:
    for _inner, _node in INNER_INLINES:
        case(f"in/{_outer}<{_inner}", _build(_node))

BLOCK_IN = [
    ("BlockQuote", lambda bs: {"t": "BlockQuote", "c": bs}),
    ("Div", lambda bs: {"t": "Div", "c": [attr("", ["d"]), bs]}),
    ("Bullet", lambda bs: {"t": "BulletList", "c": [bs]}),
    ("Ordered", lambda bs: {"t": "OrderedList",
                            "c": [[1, {"t": "Decimal"}, {"t": "Period"}], [bs]]}),
    ("Definition", lambda bs: {"t": "DefinitionList", "c": [[inls("term"), [bs]]]}),
    ("Note", lambda bs: {"t": "Para", "c": [S("a"), {"t": "Note", "c": bs}]}),
    ("Cell", lambda bs: _table_of(bs)),
]

INNER_BLOCKS = [
    ("CodeBlock", [{"t": "CodeBlock", "c": [attr("", ["bash"]), "x"]}]),
    ("BulletList", [{"t": "BulletList", "c": [[plain("a")], [plain("b")]]}]),
    ("Table", [table(["AlignDefault"] * 2, [["h1", "h2"]], [["a", "b"]])]),
    ("BlockQuote", [{"t": "BlockQuote", "c": [para("q")]}]),
    ("Header", [{"t": "Header", "c": [2, A, inls("H")]}]),
    ("HorizontalRule", [{"t": "HorizontalRule"}]),
    ("LineBlock", [{"t": "LineBlock", "c": [inls("one"), inls("two")]}]),
    ("Para.two", [para("one"), para("two")]),
]

for _outer, _build in BLOCK_IN:
    for _inner, _blocks in INNER_BLOCKS:
        case(f"in/{_outer}<{_inner}", _build(_blocks))

# **Letters and digits only.** `@@%d@@` looked unambiguous and was not:
# this writer escapes an `@` that follows an alphanumeric, to keep gfm
# from autolinking an address pandoc loses — so every sentinel came back
# as `@@7\@@`, no case was ever found, and gfm reported a perfect score
# it had never been measured for. A sentinel must be chosen to survive
# every writer, not to force one to change.
SENTINEL = "zzcasezz%dzz"

def document(cases):
    blocks = []
    for n, (_, b) in enumerate(cases):
        blocks.append({"t": "Para", "c": [S(SENTINEL % n)]})
        blocks.append(b)
    blocks.append({"t": "Para", "c": [S(SENTINEL % len(cases))]})
    return json.dumps({"pandoc-api-version": [1, 23, 1], "meta": {}, "blocks": blocks})

def run(cmd, data):
    r = subprocess.run(cmd, input=data, capture_output=True, text=True)
    return r.stdout

MARK = re.compile(r"zzcasezz(\d+)zz")

def split(text, n):
    """Cut the rendered document back into per-case pieces.

    The sentinel is found **anywhere in the line**, not as the whole
    line: every writer dresses a paragraph differently and HTML wraps it
    in `<p>`. Requiring the bare line silently matched nothing there, so
    every case fell into one bucket, no case ever differed, and the
    writer reported a perfect score it had not earned.
    """
    out, current, seen = [], [], -1
    for line in text.split("\n"):
        found = MARK.search(line)
        if found:
            index = int(found.group(1))
            if seen >= 0:
                out.append("\n".join(current).strip("\n"))
            current, seen = [], index
            if index != len(out):
                return None            # misaligned: refuse rather than guess
            continue
        if seen >= 0:
            current.append(line)
    return out if len(out) == n else None

def sweep(writer, ours, theirs):
    doc = document(CASES)
    p = run(["pandoc", "-f", "json", "-t", theirs, "--wrap=none", "--columns=72"], doc)
    f = run([FERRODOC, "-f", "json", "-t", ours, "--wrap=none", "--columns=72"], doc)
    ps, fs = split(p, len(CASES)), split(f, len(CASES))
    if ps is None or fs is None:
        # **Never an empty list here.** Returning "no disagreements" for a
        # batch that could not be read reports a perfect score for a run
        # that measured nothing, which is how this tool first claimed
        # `html 137/137`.
        print(f"{writer}: SENTINELS MISALIGNED — this batch measured nothing", file=sys.stderr)
        sys.exit(2)
    return [(name, a, b) for (name, _), a, b in zip(CASES, ps, fs) if a != b]

# **A floor per writer, and it is a contract, not a high-water mark.**
# Unlike `real-world.sh` this corpus cannot drift: the AST is a fixed set
# of variants and pandoc is pinned, so a number that falls here is always
# a regression. Raise a floor when a fix lands; never lower one.
# **Divergences this project has decided to keep**, each recorded in
# COMPATIBILITY.md with a repro. They are reported separately rather
# than hidden: a construct listed here is not work, and a construct that
# stops matching for a *second* reason still shows up as a disagreement,
# because the comparison is unchanged — only the label moves.
DELIBERATE = {
    # Pandoc writes the content `x\n` as `x` and reads it back as `x`.
    ("markdown", "block/CodeBlock.newline"),
    ("commonmark", "block/CodeBlock.newline"),
    ("gfm", "block/CodeBlock.newline"),
    # Pandoc writes `a@b.com` bare and its own reader linkifies it.
    ("gfm", "escape/@"),
    # The same rule reached from another construct: `[-@k]` is a
    # `SuppressAuthor` citation rendered as text, and the `@` after the
    # `-` is escaped here for the same reason. Narrowing the escape to
    # require a domain would close both, and would also give up the
    # address pandoc loses.
    ("gfm", "inline/Cite.suppress-author"),
    # `\setcounter` before `\def`: pandoc's reader takes the start value
    # from the first directive it meets, so pandoc's order loses it.
    ("latex", "block/OrderedList.start3"),
    # Pandoc runs out of RST underline characters at level 6 and writes
    # a line of **spaces**, which is not an underline: its own reader
    # gives the heading back as a paragraph. Five quotes survive.
    ("rst", "block/Header6"),
    # **A list marker on the line after a two-space hard break.** Pandoc
    # writes it bare and its own reader then takes it as a list: `a  \n- x`
    # comes back a `Para` *and* a `BulletList`, and `1.` the same way. The
    # escape here keeps the one paragraph the document is. `===` is the
    # same rule seen from the setext end and is escaped with them.
    #
    #     printf 'a  \n- x\n' | pandoc -f commonmark -t json
    *[(flavour, f"escape/continuation {lead!r}")
      for flavour in ("commonmark", "gfm")
      for lead in ("-", "+", "1.", "1)", "2.", "===")],
    # **A `LineBreak` inside an inline container.** Markdown has no
    # spelling for one, and pandoc's attempt does not survive its own
    # reader: `*\*` comes back as the `Str` `"**"` for the dialect, and a
    # nested `BulletList` for the other two. What this writes round-trips
    # exactly in the dialect and comes back as literal asterisks with the
    # break intact in the other two — less lost, either way.
    #
    #     ./scripts/probe.sh -t markdown -f markdown 'para(emph(linebreak))'
    *[(flavour, f"in/{holder}<LineBreak")
      for flavour, holders in (
          ("markdown", ("Emph", "Header", "Term", "Cell")),
          ("commonmark", ("Emph", "Link", "Header", "Quoted", "Term")),
          ("gfm", ("Emph", "Link", "Header", "Quoted", "Term", "Cell")),
      )
      for holder in holders],
    # **A footnote of more than one block in AsciiDoc.** The syntax is
    # `footnote:[...]`, an inline macro, and a block cannot go in one —
    # pandoc gives up and writes `[multiblock footnote omitted]`, losing
    # the content outright. This writes the blocks into the macro, which
    # AsciiDoctor renders and pandoc does not read back either way.
    # Already recorded before the sweep saw it; see COMPATIBILITY.md.
    *[("asciidoc", f"in/Note<{inner}")
      for inner in ("CodeBlock", "BulletList", "Table", "BlockQuote",
                    "Header", "HorizontalRule", "LineBlock", "Para.two")],
}

# **Two axes, two floors, because one scalar over both gates neither.**
# Under a single floor over all 276 cases a construct from the flat axis
# can regress while a composition case is fixed and the total holds — the
# same defect as a percentage over a large corpus, which this project
# already refuses elsewhere. The flat floors are the whole-alphabet
# contract every writer had already met; the composition floors start
# where the cross product first found them and rise from there.
def group_of(name):
    return "composition" if name.startswith("in/") else "flat"

FLOORS = {"markdown": 159, "commonmark": 158, "gfm": 160, "html": 158,
          "latex": 157, "rst": 160, "asciidoc": 160, "plain": 158}
COMPOSITION = {"markdown": 120, "commonmark": 116, "gfm": 114, "html": 118,
               "latex": 106, "rst": 83, "asciidoc": 103, "plain": 102}
GROUPS = [("flat", FLOORS), ("composition", COMPOSITION)]

FERRODOC = "./target/release/ferrodoc"
ARGS = sys.argv[1:]
FLOORS_ONLY = "--floors" in ARGS
BLESS = "--bless" in ARGS
ARGS = [a for a in ARGS if a not in ("--floors", "--bless")]
if ARGS and "/" in ARGS[0]:
    FERRODOC = ARGS.pop(0)

WRITERS = [(w, w, w) for w in
           ["markdown", "commonmark", "gfm", "html", "latex", "rst", "asciidoc", "plain"]]
if ARGS:
    WRITERS = [w for w in WRITERS if w[0] in ARGS]

total, below = 0, 0
summary = []
scores = {}
for writer, ours, theirs in WRITERS:
    found = sweep(writer, ours, theirs)
    kept = [f for f in found if (writer, f[0]) in DELIBERATE]
    bad = [f for f in found if (writer, f[0]) not in DELIBERATE]
    total += len(bad)
    shown = False
    parts = []
    for group, table in GROUPS:
        size = sum(1 for name, _ in CASES if group_of(name) == group)
        missed = [f for f in bad if group_of(f[0]) == group]
        score = size - len(missed)
        scores[(group, writer)] = score
        parts.append(f"{score}/{size}")
        floor = table.get(writer, 0)
        if score < floor:
            below += 1
            print(f"=== {writer} {group}: {score}/{size} — BELOW ITS FLOOR OF {floor}")
        elif not FLOORS_ONLY:
            print(f"\n=== {writer} {group}: {score}/{size} constructs identical")
        if FLOORS_ONLY and score >= floor:
            continue
        shown = True
        for name, p, f in missed:
            print(f"  {name}")
            print(f"      pandoc: {p!r}")
            print(f"      ours:   {f!r}")
    summary.append(f"{writer} {'+'.join(parts)}")
    if shown:
        for name, _, _ in kept:
            print(f"  {name} — deliberate, see COMPATIBILITY.md")

# **`--bless` rewrites the floors from this run**, which is the one edit
# here that is pure transcription — eight of them by hand in a day, and a
# floor typed one too low is a gate that cannot fail. It only ever raises
# one: a floor that would fall is a regression, and the answer to that is
# never to write the smaller number down.
if BLESS:
    source = open(__file__, encoding="utf-8").read()
    raised = []
    for name, group, table in (("FLOORS", "flat", FLOORS),
                               ("COMPOSITION", "composition", COMPOSITION)):
        moved = False
        for writer in list(table):
            score = scores.get((group, writer))
            if score is not None and score > table[writer]:
                raised.append(f"{writer} {group} {table[writer]} -> {score}")
                table[writer] = score
                moved = True
        if not moved:
            continue
        body = (",\n" + " " * (len(name) + 4)).join(
            ", ".join(f'"{w}": {table[w]}' for w in half)
            for half in (list(table)[:4], list(table)[4:])
        )
        start = source.index(name + " = {")
        end = source.index("}", start) + 1
        source = source[:start] + name + " = {" + body + "}" + source[end:]
    if raised:
        open(__file__, "w", encoding="utf-8").write(source)
        print("raised: " + ", ".join(raised))
    else:
        print("no floor moved")

# One line last, because `verify.sh` reports a gate by its final line.
print(f"identical to pandoc on {len(CASES)} AST constructs: " + ", ".join(summary))
sys.exit(1 if below else 0)
