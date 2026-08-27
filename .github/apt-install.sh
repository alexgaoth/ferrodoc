#!/usr/bin/env bash
# Install Ubuntu packages on a runner, without the runner's *other* apt
# sources deciding whether this job passes.
#
# The GitHub image carries third-party sources — Microsoft's azure-cli and
# prod repositories among them — that answer 403 often enough to matter,
# and one unreachable source fails the whole update:
#
#   E: Failed to fetch https://packages.microsoft.com/repos/azure-cli/...
#      403  Forbidden [IP: 13.107.213.66 443]
#   ##[error]Process completed with exit code 100.
#
# That took CI red on the 0.7.0 release commit, with nothing wrong in the
# tree. Nothing this repository installs comes from those repositories, so
# they are dropped before asking; the update is then retried, because the
# Ubuntu archive has bad minutes of its own.
#
# Usage: .github/apt-install.sh valgrind asciidoctor …
set -euo pipefail

sudo rm -f /etc/apt/sources.list.d/*microsoft* /etc/apt/sources.list.d/*azure* || true

for attempt in 1 2 3; do
  if sudo apt-get update; then
    break
  fi
  if [ "$attempt" = 3 ]; then
    echo "::error::apt-get update failed three times — this is the runner's" \
         "package mirrors, not this repository"
    exit 1
  fi
  echo "apt-get update failed (attempt $attempt of 3); retrying in 15s"
  sleep 15
done

sudo apt-get install -y --no-install-recommends "$@"
