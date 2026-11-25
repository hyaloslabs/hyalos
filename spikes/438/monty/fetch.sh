# Sourced by build.sh and typecheck.sh (Slice 4 of docs/plans/hyalos-kernel-python): fetches
# Monty's release tag v1.6.0 (the tag the plan names; it exists) into the spike's cache and stops
# unless the tag is the pinned commit. A tree fetched before is reused when its commit is the
# pinned one, so a later run needs no network for Monty itself. Sets `src` and `cache`, and needs
# `fail`.
URL=https://github.com/pydantic/monty
TAG=v1.6.0
SHA=3f89dcbf82071896efb5e3ded507db4191e8d829
TOOLCHAIN=1.99.1

cache=${HYALOS_SPIKE_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/hyalos-spike-438}
src="$cache/monty-src"

# A cargo run that calls these scripts must not hand its own settings to their builds.
for name in $(env | sed -n 's/^\(CARGO[A-Z_]*\|RUSTFLAGS\|RUSTC[A-Z_]*\|RUSTUP_TOOLCHAIN\)=.*/\1/p'); do
  [ "$name" = CARGO_HOME ] || unset "$name"
done

mkdir -p "$cache"
if [ "$(git -C "$src" rev-parse -q --verify HEAD 2>/dev/null || true)" != "$SHA" ]; then
  rm -rf "$src"
  git init -q "$src"
  git -C "$src" fetch -q --depth 1 "$URL" "refs/tags/$TAG:refs/tags/$TAG" \
    || fail "cannot fetch $TAG from $URL"
  got=$(git -C "$src" rev-parse "$TAG^{commit}")
  [ "$got" = "$SHA" ] || fail "$TAG is $got at $URL, not the pinned $SHA"
  git -C "$src" checkout -q --detach "$SHA"
fi
[ "$(git -C "$src" rev-parse HEAD)" = "$SHA" ] || fail "the fetched tree is not at $SHA"

# Every registry package in lockfile $1 is at the version Monty's own Cargo.lock pins.
lock_matches() {
  python3 -I - "$src/Cargo.lock" "$1" <<'PY'
import sys, tomllib
def pins(path):
    with open(path, "rb") as f:
        return {(p["name"], p["version"]) for p in tomllib.load(f)["package"] if "source" in p}
extra = sorted(pins(sys.argv[2]) - pins(sys.argv[1]))
if extra:
    sys.exit("%s pins packages Monty's Cargo.lock does not: %s" % (sys.argv[2], extra))
PY
}
