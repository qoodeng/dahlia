#!/bin/bash
# Turns a finished ship run into a pull request for its bead.
# Run by .github/workflows/factory.yml from the runner's checkout of the default branch.
#   open-pr.sh <run-id> <bead-id>   (ISSUE_TITLE and DEFAULT_BRANCH in the environment)
# Prints the PR's URL last. Its branch is factory/<bead-id>, which factory-close.yml reads to
# close the bead when the PR merges.
# The branch and PR are made with FACTORY_TOKEN, the owner's fine-grained token (contents,
# pull requests and issues; no workflows). Events from the job's own GITHUB_TOKEN start no
# workflows, and GitHub treats its PRs as an outside contributor's, so CI would never run
# unapproved. With the owner's token the PR starts CI like any other, and non-visual changes
# get GitHub auto-merge, which merges once the required checks pass. factory-close.yml closes
# the bead when the PR merges.
set -euo pipefail

run_id="$1"
bead="$2"
title="${ISSUE_TITLE:?}"
base=$(git rev-parse HEAD)
branch="factory/$bead"

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
    commit -q -m "$title" -m "Bead $bead. Made by Fabro run $run_id."
auth=$(printf 'x-access-token:%s' "${FACTORY_TOKEN:?}" | base64)
git -c http.https://github.com/.extraheader="AUTHORIZATION: basic $auth" \
    push --force -q https://github.com/"$GITHUB_REPOSITORY".git "HEAD:refs/heads/$branch"

# A missing verdict counts as visual, so nothing merges unseen.
visual=$(tr -d '[:space:]' <"$workspace/.factory/visual" 2>/dev/null || true)
if [ "$visual" = "no" ]; then
    label="factory-automerge"
    merge_note="Not visual: merges itself once CI passes."
else
    label="needs-visual-review"
    merge_note="Visual (or unsure): waits for you. Add the factory-automerge label to approve; it merges once CI passes."
fi

body="$RUNNER_TEMP/factory-$run_id-body.md"
{
    echo "Bead \`$bead\`"
    echo
    echo "Fabro run \`$run_id\`. $merge_note"
    echo
    echo "## Review"
    echo
    head -c 50000 "$workspace/.factory/review.md" 2>/dev/null || echo "No review notes."
} >"$body"

url=$(GH_TOKEN="$FACTORY_TOKEN" gh pr create --repo "$GITHUB_REPOSITORY" --base "${DEFAULT_BRANCH:?}" \
    --head "$branch" --title "$title ($bead)" --body-file "$body" --label "$label")
if [ "$label" = "factory-automerge" ]; then
    GH_TOKEN="$FACTORY_TOKEN" gh pr merge "$branch" --repo "$GITHUB_REPOSITORY" --auto --squash
fi
echo "$url"
