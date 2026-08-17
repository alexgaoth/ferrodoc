#!/usr/bin/env bash
# The CommonMark spec, in chunks of 30 examples, as EPUBs pandoc wrote.
#
# Kept apart from `corpus/epub` because it measures something different.
# Each file bundles 30 examples, so *one* of the HTML reader's 26 known
# divergences (`diff-html-read`, 633/659) fails the whole document — and
# with 30 examples per file, most files contain one. The score here is
# therefore a compounding of a divergence already measured elsewhere, and
# averaging it into the EPUB gate would report the HTML reader's fidelity
# under the EPUB reader's name.
#
# It still earns its place: it is the only thing exercising the spine and
# the identifier prefixing at volume, and a *drop* here is a real
# regression even though the level is not a fidelity claim.
set -euo pipefail
cd "$(dirname "$0")"

python3 - <<'PY'
import json, subprocess
spec = json.load(open('../commonmark-spec-0.31.2.json'))
mds = [e['markdown'] for e in spec]
for i in range(0, len(mds), 30):
    md = '\n\n'.join(mds[i:i+30])
    subprocess.run(['pandoc', '-f', 'markdown', '--split-level=1',
                    '-o', f'spec-{i//30:02d}.epub'], input=md, text=True, check=True)
PY
