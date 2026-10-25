#!/bin/sh
# Build, notarize, tag and publish a Dahlia release from this Mac. Rehearse with --dry-run.
set -eu
exec python3 "$(dirname "$0")/release.py" "$@"
