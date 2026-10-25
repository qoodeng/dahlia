#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
git submodule update --init vendor/markraft
python3 vendor/markraft/scripts/prepare-dependencies.py
# Applied in this order. Later patches change lines earlier ones add, so an earlier
# patch no longer reverses cleanly once a later one is in: find the last patch already
# applied (the one that reverses cleanly on top) and apply only the ones after it.
set -- markraft-code-font.patch markraft-task-marker.patch markraft-image-confinement.patch markraft-find-results.patch markraft-batch-corrections.patch markraft-batch-source.patch markraft-linear-find.patch markraft-prepared-edit.patch markraft-scaled-style.patch markraft-prepared-setup.patch markraft-native-quit.patch markraft-maintained-selectors.patch markraft-native-window-menu.patch
applied=0
index=0
for patch in "$@"; do
    index=$((index + 1))
    if git -C vendor/markraft apply --reverse --check "../../patches/$patch" 2>/dev/null; then
        applied=$index
    fi
done
index=0
for patch in "$@"; do
    index=$((index + 1))
    if [ "$index" -gt "$applied" ]; then
        git -C vendor/markraft apply --check "../../patches/$patch"
        git -C vendor/markraft apply "../../patches/$patch"
    fi
done

# Owned patches can also amend the generated native dependency.
python3 vendor/markraft/scripts/prepare-dependencies.py

python3 scripts/prepare-maintained-dependencies.py
