#!/bin/sh
# Fails when vendor/markraft differs from its pinned SHA plus the owned patches.
# `.gitmodules` sets `ignore = dirty`, so git status alone never shows this drift.
set -eu
cd "$(dirname "$0")/.."
submodule=vendor/markraft
pinned=$(git ls-tree HEAD "$submodule" | awk '{print $3}')
head=$(git -C "$submodule" rev-parse HEAD)
if [ "$head" != "$pinned" ]; then
    echo "error: $submodule is at $head, expected pinned $pinned" >&2
    exit 1
fi
untracked=$(git -C "$submodule" ls-files --others --exclude-standard)
if [ -n "$untracked" ]; then
    echo "error: untracked files in $submodule:" >&2
    echo "$untracked" >&2
    exit 1
fi
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
# Expected tree: the pinned commit with each patch applied in order, built in a private index.
GIT_INDEX_FILE="$scratch/expected" git -C "$submodule" read-tree "$pinned"
for patch in markraft-code-font.patch markraft-task-marker.patch markraft-image-confinement.patch markraft-find-results.patch markraft-batch-corrections.patch markraft-batch-source.patch markraft-linear-find.patch markraft-prepared-edit.patch markraft-scaled-style.patch markraft-prepared-setup.patch markraft-native-quit.patch markraft-maintained-selectors.patch markraft-native-window-menu.patch; do
    GIT_INDEX_FILE="$scratch/expected" git -C "$submodule" apply --cached "../../patches/$patch"
done
expected=$(GIT_INDEX_FILE="$scratch/expected" git -C "$submodule" write-tree)
# Actual tree: the pinned commit with every tracked working-copy change.
GIT_INDEX_FILE="$scratch/actual" git -C "$submodule" read-tree "$pinned"
GIT_INDEX_FILE="$scratch/actual" git -C "$submodule" add -u
actual=$(GIT_INDEX_FILE="$scratch/actual" git -C "$submodule" write-tree)
if [ "$expected" != "$actual" ]; then
    echo "error: $submodule has drifted from patches/. Differences (expected -> working copy):" >&2
    git -C "$submodule" --no-pager diff --stat "$expected" "$actual" >&2
    git -C "$submodule" --no-pager diff "$expected" "$actual" >&2
    exit 1
fi
echo "vendor/markraft matches $pinned plus patches/."
