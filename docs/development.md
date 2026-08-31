# Developing ferrodoc

Two contracts govern this project: output must be value-identical to
pandoc's, proven differentially and never assumed; and every advantage
claimed to a reader must be a number someone else can reproduce.

`docs/gates.md` says what each gate measures and what a green run does not
prove. This file is the other half — **the method**: how a rule about
pandoc gets established, how a measurement earns the right to be called a
gate, and the specific mistakes that have cost this project days. Nothing
below is a style preference. Each line is here because ignoring it
produced a wrong number, a red `main`, or a divergence recorded that never
existed.

## Measuring

**A gate cannot fail on a construct its corpus does not contain.** This is
the most expensive lesson here — seven bugs shipped with every gate green.
`diff-html` scores the CommonMark spec, which has no tables. `diff-md`
round-trips through this project's own reader, so an inline it never
produces never reaches the writer. Column alignment, column widths, five
inline types, `$x$`, footnotes, heading attribute order and a skipped
heading level all passed that way. Ask what a corpus omits *before*
believing its score, and add the construct rather than trusting the number.

**Where the input space has a finite grammar, enumerate it; sample only
where the space is open.** `scripts/ast-sweep.sh` exists because the
Pandoc AST is a closed set of variants: walking it names every construct
two writers disagree on, in one pass, by name. It found the `plain` writer
failing 33 of 137 constructs while that same writer scored 38/40 on
documents, and 19 ordered-list marker styles no document contained. A
document corpus could not have found either, however large.

**Enumeration covers the alphabet, not the sentences.** The sweep is
exhaustive over *constructors* and not over their composition: it does not
cover every attribute value, every nesting of one construct inside
another, or any reader grammar. That is a precise statement of where the
next blind spot is, and it is generable rather than collectable — the
answer is another axis, not more hand-picked cases. Do not read the sweep
as proof of general compatibility.

**A tool that measures nothing must say so, not return "no problems".**
The sweep once returned an empty disagreement list when its sentinels
misaligned, which reads identically to a perfect score: `gfm` and `html`
were published at 137/137 having never been measured, and were 129 and
128. It now exits non-zero on misalignment. A sentinel must also *survive*
every writer under test — `@@N@@` was eaten by the gfm `@` escape and by
HTML's `<p>`.

**A score you raise and a floor you leave behind is a score that will
decay.** The drop-in corpus went 26 → 33 of 48 across five commits while
its threshold stayed at 11, which permitted 22 passing commands to regress
unnoticed. Raising a measurement is half the work; the other half is
deciding which of the new results are the supported contract and moving
the floor to it. Every threshold lives in `scripts/verify.sh` and nowhere
else, so there is exactly one line to change.

**A conformance suite must distinguish "not done" from "decided
differently".** Without that split a score stalls and cannot tell you
whether you are blocked or finished. The sweep carries a `DELIBERATE` set
and `COMPATIBILITY.md` carries a reproducing command per divergence; a
difference is one or the other, never unclassified.

**Effort follows the scoreboard, so check the scoreboard points at the
value.** Writer work dominated several days of this project because the
writers had a sweep that produced a moving number and the readers did not.
Meanwhile the default `.md` path — the behaviour most users get — was the
largest open gap. A measurement that moves easily is not thereby the
measurement worth moving.

**A curated corpus is fitted to eventually, however honestly chosen.**
`highlight.sh` held 26/26 for weeks over lines picked *because* nobody
wrote them to be highlighted, while the same highlighters matched pandoc
on one system header in 40. Run `scripts/real-world.sh` after any
highlighter change; it reports rather than gates, because its corpus
drifts with the machine.

**Check that both sides of a comparison ask the same question.**
`writers.sh` compared `-t markdown` against pandoc's markdown-*dialect*
writer while this project's `markdown` still meant CommonMark, so it
reported a dialect gap as a writer's score and buried three real losses;
3/12 became 8/12 the day a `commonmark` row was added beside it. When a
default changes, re-read every gate that runs both tools and ask which
dialect each side receives.

## Establishing a rule about pandoc

**Never guess pandoc's behaviour. Probe the pinned binary, then encode the
rule with a comment saying what was observed.** Conformance is pinned to
pandoc 3.8.2.1; pandoc's GitHub sources describe a later pandoc and
disagree with the binary, so read them for algorithm shape only.

**Probe one construct per invocation, and never normalize whitespace in
what you compare.** A batch of 200 bash words came back misaligned and
would have coloured 69 of them wrongly. A `re.sub(r'\s+', ' ')` over `-t
native` turned "three spaces survive" into "one space", and two ODT rules
were derived backwards from it. A repro that gets committed must run
exactly as printed, or it is a claim rather than evidence.

**One shape is not a rule.** RST `:literal:` was recorded as "pandoc loses
a backtick here" on the strength of a single probe that happened to use an
*interior* backtick; pandoc's actual rule is about backticks that could
open or close markup. That wrong entry stood in the compatibility table
for days, and correcting it won two documents. A single observation
confirms an instance — a rule needs the shape probed on both sides of its
boundary.

The same mistake, made again while this file was being written: the
markdown writer escaped `128\.` and `\- x` on a paragraph's later lines
where pandoc writes them bare. Probed after a **hard** break, pandoc's
bare form round-tripped perfectly, so the escape looked like dead bytes
and was dropped for every line start. A **soft** break is a different
position — a list *can* open there — and `> foo` / `    - bar` promptly
came back as a blockquote holding `foo` alone. `diff-md` fell 652/652 to
649/652. The boundary that needed probing was the one between the two
kinds of break, and only one side of it had been looked at.

**When one condition guards two rules, the position test usually belongs
to only one of them.** That same escape arm bundled "a list marker opens
a block" with "a run of dashes is a setext underline" — the first is true
only on a block's first line and the second only on a line after it, so
no single test could be right for both, and the wider one silently won.
Splitting the arm fixed the narrow case without touching the deliberate
one.

**Pandoc is the oracle, not the authority: round-trip its own output
through its own reader before matching it byte for byte.** Its
`commonmark` writer loses a code block that opens a blockquote or a list
item, and its `<!-- -->` list separator is a `RawBlock` its reader hands
back, so two blocks return three. Pandoc round-trips 593 of the 652
CommonMark spec examples where this project round-trips 652. Matching the
bytes there would have meant copying the loss. When the reference
implementation loses information, you have a decision to record, not a bug
to fix — four divergences here turned on exactly that check, which
`scripts/probe.sh -f READER` performs.

**Build the tool when the boilerplate is where the mistakes live** — not
merely when the work is repetitive. Every rule here was derived by handing
one small AST to both binaries, which was fifteen lines of inline JSON per
question, and the errors were all in the fifteen lines: a `Row` written as
a tagged object where the schema wants an array, and twice a case that
measured the surrounding batch rather than the construct. That is
`scripts/probe.sh` now, and the class of error is gone.

## Writing Rust in this workspace

Every bug that survived the compiler here had the same shape: **the same
type carrying a different meaning**. Rust checks structure; it does not
check convention.

**`match` is exhaustiveness-checked and `==` is not, so adding an enum
variant is only safe where the compiler can see it.** Adding
`Flavour::Ipynb` cost the notebook round trip 11/16 → 6/16, because one
site asked `flavour == Flavour::Gfm` and silently gave the new variant
CommonMark's escaping. Nothing else moved, and no test named it. Where a
convention is consulted in more than one place, make it a method —
`Flavour::is_gfm()` exists so those two sites cannot drift again.

**A total-looking type can still carry a sentinel; ask the producer what
each inhabitant means, not what the type permits.** `Wrap::None` reaches
the markdown writer as `Some(usize::MAX)`, not `None`. Multiplied by a
table column's fraction it requested an eight-exabyte string and aborted,
and read as `columns.is_some()` meaning "can fill" it took the wrong
layout branch. The break opportunity is spelled `'\0'` and has caused
three separate bugs. Neither failure mode is a panic at the mistake — both
are arithmetic that keeps going.

**Attribute a regression by building the suspected parent commit, not by
reasoning about it.** A `git worktree` of the previous commit answers
"which change cost this" in one command. Reasoning produces plausible
stories, and the notebook regression above had two convincing wrong ones.

**Prefer the surgical edit to the clever one.** Regex-splicing Rust source
broke this codebase five times, with a recognisable signature: a doubled
match arm (`Inline::Math Inline::Math(…)`). Use exact string replacement.

## Landing a change

**Do not express a decision as a shell chain.** `./scripts/verify.sh &&
git commit … && git push` binds the push to the *commit*, not to the gate,
and has pushed a red `main` out of here. Use `./scripts/land.sh -m "…"`,
which runs the quick gate, then the full gate, then whatever `--wasm`,
`--c` or `--slow` you asked for, refuses to commit if any is red, and only
then pushes.

**Never read a gate result through a pipe, and identify a CI run by its
SHA rather than by recency.** `| tail` masks the exit status, which is how
a failing publish once read as success. A "wait for the latest run" loop
matched the run *before* the one it had just caused, and `gh run watch
--exit-status` has exited 0 on a run that concluded `failure`.

**A red that is somebody else's bad minute is fixed, not re-run.**
Re-running teaches you not to read reds, which is how a real failure sat
behind an expected one for four days.
