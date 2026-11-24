#!/usr/bin/env bash
# Whether commit $1 has its release evidence (ADR-0013, ADR-0111): a run of the ci workflow on a push
# to main, for that commit, in which every evidence job succeeded. The evidence is the simulation and
# its fault sweep, the job spec's model check, the contract suites and the red-team corpus (both in
# the rest of the workspace's tests), the chaos scenarios (in the hyalos crate's tests) and the
# required check that gathers them. Run with GH_TOKEN and GITHUB_REPOSITORY set. Job names after the
# commit replace that list, for a release that needs other evidence (the verifier's, ADR-0131).
#
# It fails closed: a run still going, a job skipped, cancelled or failed, or an answer it cannot read
# is no evidence. It only reads; it tags nothing and publishes nothing.
set -euo pipefail

sha="${1:?usage: release-evidence.sh <commit> [ci job name ...]}"
shift
repo="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is not set}"
[[ "$sha" =~ ^[0-9a-f]{40}$ ]] || { echo "not a commit id: $sha"; exit 1; }

# The ci jobs a release needs, by name. crates/xtask/tests/release_gate.rs checks each is a ci job.
EVIDENCE=(
  "simulation"
  "job spec"
  "test (the hyalos crate)"
  "test (the rest of the workspace)"
  "build, lint, test"
)
if [[ $# -gt 0 ]]; then
  EVIDENCE=("$@")
fi

runs=$(gh api "repos/$repo/actions/runs?head_sha=$sha&event=push&branch=main&per_page=100" \
  --jq '.workflow_runs[] | select(.path == ".github/workflows/ci.yml") | "\(.id) \(.status) \(.conclusion)"')
if [[ -z "$runs" ]]; then
  echo "no ci run on main for $sha: no evidence"
  exit 1
fi

while read -r id status conclusion; do
  if [[ "$status" != "completed" || "$conclusion" != "success" ]]; then
    echo "ci run $id: $status, $conclusion"
    continue
  fi
  # The latest attempt's jobs, as name<TAB>conclusion.
  jobs=$(gh api "repos/$repo/actions/runs/$id/jobs?filter=latest&per_page=100" \
    --jq '.jobs[] | "\(.name)\t\(.conclusion)"')
  missing=()
  for name in "${EVIDENCE[@]}"; do
    grep -qxF "$name"$'\t'"success" <<<"$jobs" || missing+=("$name")
  done
  if [[ ${#missing[@]} -eq 0 ]]; then
    echo "evidence for $sha: ci run $id, every evidence job succeeded:"
    printf '  %s\n' "${EVIDENCE[@]}"
    exit 0
  fi
  echo "ci run $id: no success for: ${missing[*]}"
done <<<"$runs"

echo "no ci run on main for $sha has every evidence job green: no evidence"
echo "If ci is still running, re-run this workflow once it has finished."
exit 1
