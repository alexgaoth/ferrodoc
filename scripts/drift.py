#!/usr/bin/env python3
"""Every threshold the prose asserts must be a threshold a gate has.

`scripts/verify.sh` is the single source of every threshold, and that
rule has held. What drifted is the **prose about** them: on 2026-09-02
three files still explained a drop-in threshold of 33 that the gate had
left behind at 47 — the floor was right, and every sentence describing it
was two moves stale. Nothing could have caught that, because a gate
checks scores and no gate reads its own documentation.

So this reads the thresholds out of the scripts that enforce them, reads
every number the prose asserts *is* a threshold, and fails when the prose
names one that no gate has.

**`ROADMAP.md` and `CHANGELOG.md` are exempt, and deliberately.** Both
are history by their own reading discipline — "historical numbers are not
current claims" — and a check that forbade `it sat at 11` would forbid
explaining how a floor got where it is.

    ./scripts/drift.sh            every claim, and every one adrift
    ./scripts/drift.sh --floors   silent unless a claim is adrift
"""
import re, sys, glob, pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent

# Where a threshold is actually enforced.
SOURCES = {
    "scripts/verify.sh":     [r"--fail-under ([\d.]+)"],
    "scripts/math.py":       [r"^FLOOR = (\d+)"],
    "scripts/ast-sweep.py":  [r": (\d+)"],
    "scripts/writers.sh":    [r"echo (\d+) ;;"],
}

# Where a threshold is described. Anything asserting a number here is a
# claim about a gate, and has to match one.
CLAIMS = ["README.md", "COMPATIBILITY.md", "dropin/README.md",
          "scripts/verify.sh", "scripts/writers.sh", "scripts/dropin.sh",
          "scripts/math.sh", "scripts/ast-sweep.sh", "scripts/hostile.sh"]
CLAIMS += sorted(glob.glob("docs/*.md"))

ASSERTS = [
    r"threshold (?:is|of|at) ([\d.]+)",
    r"floor (?:is|of|at) ([\d.]+)",
    r"gated at ([\d.]+)",
    r"--fail-under ([\d.]+)",
]


def authoritative():
    found = set()
    for path, patterns in SOURCES.items():
        text = (ROOT / path).read_text()
        # ast-sweep's floors live in two dicts; take only those lines.
        if path.endswith("ast-sweep.py"):
            text = "\n".join(l for l in text.splitlines()
                             if re.match(r"\s*(FLOORS|COMPOSITION)?\s*[=\{]|^\s*\"", l)
                             and ":" in l)
        for pattern in patterns:
            found |= set(re.findall(pattern, text, re.M))
    return found


def main():
    quiet = "--floors" in sys.argv
    known = authoritative()
    adrift, checked = [], 0
    for name in CLAIMS:
        path = ROOT / name
        if not path.exists():
            continue
        for number, line in ((m.group(1), i + 1)
                             for i, text in enumerate(path.read_text().splitlines())
                             for pattern in ASSERTS
                             for m in [re.search(pattern, text)] if m):
            checked += 1
            if number not in known:
                adrift.append((name, line, number))
    if not quiet:
        for name, line, number in adrift:
            print(f"  {name}:{line} claims a threshold of {number}, which no gate has")
        print(f"{checked - len(adrift)}/{checked} stated thresholds match a gate "
              f"({len(known)} in force)")
    if adrift:
        if quiet:
            for name, line, number in adrift:
                print(f"  {name}:{line}: threshold {number} matches no gate", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
