#!/bin/sh
# Build and qualify a Developer ID distribution without launching the app.
set -eu
exec python3 "$(dirname "$0")/package-mac.py" "$@"
