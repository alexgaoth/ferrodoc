#!/usr/bin/env bash
# Gate, commit, push, and read the conclusion of the run it caused.
#
# **Two ways a red `main` has gone out of here, both process rather than
# code, and both are what this exists to stop.**
#
# `./scripts/verify.sh > log; git commit … && git push` runs the push
# whatever the gate said — the `&&` binds the push to the *commit*. That
# put a failing writer gate on `main` on 2026-08-29.
#
# And "wait for the latest run" can match the run *before* the one you
# just caused, so a green from the previous commit reads as a green for
# this one. `gh run watch --exit-status` is no better: it has exited 0 on
# a run that concluded `failure`. The only thing worth reading is the
# conclusion of the run whose head **is this commit**.
#
#   git add -- path…
#   ./scripts/land.sh -m "subject line"
#   ./scripts/land.sh --wasm --slow -F /tmp/message.txt
#
# `--wasm`, `--c` and `--slow` add the gates `verify.sh` does not run:
# a **writer** change needs `--wasm`, and any change to a published
# figure needs `--slow`. Nothing is committed unless every gate asked
# for passes, and the exit status is the CI conclusion.
set -euo pipefail
cd "$(dirname "$0")/.."

want_wasm=0 want_c=0 want_slow=0 no_wait=0
commit_args=()
while [ $# -gt 0 ]; do
    case "$1" in
        --wasm)    want_wasm=1 ;;
        --c)       want_c=1 ;;
        --slow)    want_slow=1 ;;
        --no-wait) no_wait=1 ;;
        -m|-F)     commit_args+=("$1" "$2"); shift ;;
        *) echo "usage: $0 [--wasm] [--c] [--slow] [--no-wait] (-m MSG | -F FILE)" >&2
           exit 2 ;;
    esac
    shift
done
[ "${#commit_args[@]}" -gt 0 ] || { echo "no message: pass -m or -F" >&2; exit 2; }
git diff --cached --quiet && { echo "nothing staged" >&2; exit 2; }

# Staging explicit paths is the rule here — the working tree is shared
# with other sessions — so say what is going in before the gates run.
echo "== staged"
git diff --cached --name-only | sed 's/^/   /'

run_gate() {
    local name=$1; shift
    printf '== %-28s ' "$name"
    if "$@" > /tmp/land-$$.log 2>&1; then
        echo ok
    else
        echo FAILED
        grep -nE 'FAILED|DRIFTED|BELOW|^error' /tmp/land-$$.log | head -8 | sed 's/^/   /'
        echo "   full log: /tmp/land-$$.log" >&2
        exit 1
    fi
}

# The cheap checks first: tests, clippy and the trimmed build take about
# forty seconds and catch most of what fails, where the full run is three
# minutes. A `too_many_lines` found at the end of the long one is three
# minutes spent to learn a function grew.
run_gate "quick checks" ./scripts/verify.sh --quick
run_gate "verify" ./scripts/verify.sh
[ "$want_wasm" = 0 ] || run_gate "wasm and npm" ./scripts/verify.sh --wasm
[ "$want_c" = 0 ]    || run_gate "C ABI" ./scripts/verify.sh --c
[ "$want_slow" = 0 ] || run_gate "published figures" ./scripts/claims.sh --slow
rm -f /tmp/land-$$.log

git commit -q "${commit_args[@]}"
sha=$(git rev-parse HEAD)
echo "== committed ${sha:0:7}"
git push
[ "$no_wait" = 0 ] || exit 0

echo "== waiting for CI on ${sha:0:7}"
for _ in $(seq 1 60); do
    read -r head status conclusion <<<"$(
        gh run list --limit 5 --json headSha,status,conclusion \
            --jq ".[] | select(.headSha == \"$sha\") | \"\(.headSha) \(.status) \(.conclusion)\"" \
            2>/dev/null | head -n1)"
    if [ "${status:-}" = completed ]; then
        echo "== CI ${sha:0:7}: $conclusion"
        [ "$conclusion" = success ] || {
            gh run view --json jobs \
                --jq '.jobs[] | select(.conclusion=="failure") | "   failed job: \(.name)"' \
                2>/dev/null
            exit 1
        }
        exit 0
    fi
    sleep 25
done
echo "== CI on ${sha:0:7} did not finish in 25 minutes; read it yourself" >&2
exit 1
