#!/bin/bash
# Turns a finished ship run into a pull request for its issue.
# Run by .github/workflows/factory.yml from the runner's checkout of the default branch.
#   open-pr.sh <run-id> <issue-number>   (ISSUE_TITLE and DEFAULT_BRANCH in the environment)
# The PR is pushed and opened with the owner's gh login, not the job's GITHUB_TOKEN, because
# pushes and PRs made with GITHUB_TOKEN do not start CI. Non-visual changes get the
# factory-automerge label; factory-merge.yml merges them once CI passes.
set -euo pipefail

run_id="$1"
issue="$2"
title="${ISSUE_TITLE:?}"
base=$(git rev-parse HEAD)
branch="factory/issue-$issue"

workspace=$(ls -d "$HOME"/.fabro/storage/scratch/*"$run_id"*/petri/scopes/*/work 2>/dev/null | head -1)
if [ -z "$workspace" ]; then
    echo "No workspace found for run $run_id." >&2
    exit 1
fi

patch="$RUNNER_TEMP/factory-$run_id.patch"
git -C "$workspace" diff --binary "$base" HEAD -- . ':(exclude).factory' ':(exclude).fabro' >"$patch"
if [ ! -s "$patch" ]; then
    echo "Run $run_id changed nothing."
    exit 3
fi

git switch -C "$branch"
git apply --index "$patch"
git -c user.name="Dahlia Factory" -c user.email="84674359+qoodeng@users.noreply.github.com" \
    commit -q -m "$title" -m "Closes #$issue. Made by Fabro run $run_id."
# gh's credential helper would prefer the job's read-only GH_TOKEN, so clear it for the push.
env -u GH_TOKEN -u GITHUB_TOKEN git -c credential.helper= -c 'credential.helper=!gh auth git-credential' \
    push --force -q https://github.com/"$GITHUB_REPOSITORY".git "HEAD:refs/heads/$branch"

# A missing verdict counts as visual, so nothing merges unseen.
visual=$(tr -d '[:space:]' <"$workspace/.factory/visual" 2>/dev/null || true)
if [ "$visual" = "no" ]; then
    label="factory-automerge"
    merge_note="Not visual: merges itself once CI passes."
else
    label="needs-visual-review"
    merge_note="Visual (or unsure): waits for you. Add the factory-automerge label or merge it yourself to approve."
fi

body="$RUNNER_TEMP/factory-$run_id-body.md"
{
    echo "Closes #$issue"
    echo
    echo "Fabro run \`$run_id\`. $merge_note"
    echo
    echo "## Review"
    echo
    head -c 50000 "$workspace/.factory/review.md" 2>/dev/null || echo "No review notes."
} >"$body"

env -u GH_TOKEN -u GITHUB_TOKEN gh pr create --repo "$GITHUB_REPOSITORY" --base "${DEFAULT_BRANCH:?}" \
    --head "$branch" --title "$title" --body-file "$body" --label "$label"
