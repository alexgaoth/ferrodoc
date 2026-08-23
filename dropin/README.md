# The drop-in corpus

**Not under `corpus/`, deliberately.** The gates walk that directory by
file extension, so the four HTML assets below joined `diff-html-read` the
moment they landed there and took it from 96.1% to 95.8% — a corpus
widened by accident, which is a defect this repository has had before.
This one is a corpus of *command lines*, not of documents, and nothing
walks it but `scripts/dropin.sh`.

Forty-eight pandoc command lines that people actually wrote, with the
repository each came from, so that `scripts/dropin.sh` can ask the one
question no other gate in this project asks: **if a user swapped the
binary in their Makefile, would they get the same bytes?**

Today the answer is **0 of 48**. That is the honest starting point, and
the number this repository's 1.0 criterion is written against.

## Where they came from

GitHub code search, over `Makefile`, `.yml`, `.yaml` and `.sh`:

```sh
gh search code 'pandoc -o'          --filename Makefile --limit 30
gh search code 'pandoc -s -o'       --filename Makefile --limit 30
gh search code 'pandoc --from markdown' --filename Makefile --limit 30
gh search code 'pandoc -f markdown -t'  --filename Makefile --limit 30
gh search code 'pandoc --toc'       --filename Makefile --limit 30
gh search code 'pandoc -t docx'     --filename Makefile --limit 30
gh search code 'pandoc --reference-doc' --filename Makefile --limit 30
gh search code 'pandoc --standalone'    --extension yml --limit 30
gh search code 'pandoc -t html'         --extension sh  --limit 30
gh search code 'pandoc --wrap'          --extension sh  --limit 30
gh search code 'pandoc --extract-media' --extension sh  --limit 30
gh search code 'pandoc -f docx'         --extension sh  --limit 30
gh search code 'pandoc --defaults'      --extension yml --limit 30
```

327 distinct invocations, collapsing to 34 distinct **flag signatures**.
The corpus keeps one or more real lines per signature, chosen so that the
48 rows cover the flag vocabulary in roughly the proportion the search
found it in:

| flag | occurrences |
|---|---|
| `--standalone` / `-s` | 60 |
| `--toc` | 54 |
| `--to` / `-t` | 47 |
| `--from` / `-f` | 40 |
| `--wrap` | 38 |
| `--template` | 37 |
| `--extract-media` | 37 |
| `--defaults` | 36 |
| `--output` / `-o` | 30 |
| `--css` / `-c` | 23 |
| `--toc-depth` | 14 |
| `--metadata` / `-M` | 14 |
| `--include-in-header` | 13 |
| `--metadata-file` | 12 |
| `--variable` / `-V` | 7 |
| `--number-sections` | 6 |
| `--reference-doc` | 4 |
| `--fail-if-warnings` | 3 |
| `--quiet` | 2 |

That ranking is itself a finding, and it disagrees with the order
`ROADMAP.md` had. `--template` and `--defaults` are as common as
`--wrap`; `--eol`, `--ascii`, `--strip-comments`, `--id-prefix` and
`--shift-heading-level-by` appear **zero** times in 327 real
invocations.

## What was excluded, and why

A row is admitted only if pandoc can run it here. Excluded:

- **anything needing a program this repository does not ship** —
  `--filter`, `--lua-filter`, `--citeproc`, `--bibliography`, `--csl`,
  `--pdf-engine`. A row that cannot run under pandoc alone cannot be
  compared against anything;
- **lines whose arguments are shell or Make variables** (`$@`, `$(SRC)`,
  `{{.PAPER_DIR}}`, `<input files>`) — there is no document to run them
  on;
- **continuations**, which the search returns truncated at the
  backslash.

Nothing was excluded for being awkward for ferrodoc. Fifteen of the
forty-eight use a flag ferrodoc does not have, and they are in the corpus
precisely so that the number counts them.

## What was changed, and why

27 rows are altered, and each says how in its own `changed` column. Two
kinds:

- **the target format.** A command producing PDF, `man`, `beamer` or
  `revealjs` is retargeted to the nearest format both binaries have, and
  every other flag is kept. Neither binary makes a PDF here — ferrodoc
  has no PDF writer at all and pandoc needs a TeX engine — so comparing
  the bytes of one is not possible. The flag *shape* is what the row
  tests;
- **the file the flag points at.** A `--defaults`, `--template`,
  `--css`, `--metadata-file` or `--reference-doc` in someone else's
  repository is not in this one, so `assets/` holds a stand-in of the
  same kind. The reference `.docx` is pandoc's own
  (`pandoc --print-default-data-file reference.docx`).

The `verbatim` column always holds the line exactly as it was found, so
every alteration is visible beside the original.

## The file

`commands.tsv`, tab-separated:

| column | meaning |
|---|---|
| `id` | `dropin-001` … , stable; `scripts/dropin.sh dropin-013` runs one |
| `source` | `owner/repo:path` — where the line was found |
| `input` | the document to run it on, from this repository's corpus |
| `args` | the arguments, with `%IN%` and `%OUT%` substituted at run time |
| `changed` | what was altered from the original, or `-` |
| `verbatim` | the line as found |

Adding a row means adding a real invocation with its source. A row
someone wrote to make the number move is not admissible — that is the
one rule this corpus has.
