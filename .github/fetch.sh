#!/usr/bin/env bash
# Download a file, and do not let one bad minute on somebody else's server
# decide whether this repository's tests passed.
#
# `curl -fsSL` against a GitHub release returned 500 on 2026-08-27 and took
# CI red with nothing wrong in the tree — the same shape as the apt 403 that
# `.github/apt-install.sh` exists for. Three tries with a pause between
# them; a fourth failure is a real outage and says so.
#
# Usage: .github/fetch.sh <url> <output>
set -euo pipefail
url=$1
out=$2

for attempt in 1 2 3; do
  if curl -fsSL --retry 2 --retry-delay 3 --retry-all-errors -o "$out" "$url"; then
    exit 0
  fi
  if [ "$attempt" = 3 ]; then
    echo "::error::could not download $url after three attempts — this is the" \
         "server it comes from, not this repository"
    exit 1
  fi
  echo "download of $url failed (attempt $attempt of 3); retrying in 10s"
  sleep 10
done
