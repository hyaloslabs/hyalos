#!/usr/bin/env bash
# For the operator, in their own terminal: tag a release (ADR-0013, ADR-0111). Publishing a release is
# irreversible, so a human decides it (ADR-0012). Green evidence is necessary and never sufficient:
# nothing tags a release when its checks pass, and this script creates the tag only when a human runs
# it, and only for main's head at the workspace's version with its evidence green. It does not push:
# pushing the tag it prints is the decision, and the release workflow checks the evidence again.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
git fetch --quiet origin main --tags
sha=$(git rev-parse origin/main)
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' <(git show origin/main:Cargo.toml) | head -n1)
tag="v$version"
if git rev-parse --quiet --verify "refs/tags/$tag" >/dev/null; then
  echo "$tag exists already: bump the version first (docs/operations/releases.md)"
  exit 1
fi

GITHUB_REPOSITORY="${GITHUB_REPOSITORY:-$(gh repo view --json nameWithOwner --jq .nameWithOwner)}" \
  .github/scripts/release-evidence.sh "$sha"

git tag -a "$tag" "$sha" -m "Hyalos $tag"
echo
echo "Tagged $tag at $sha, here only. To release it, push the tag:"
echo "  git push origin $tag"
