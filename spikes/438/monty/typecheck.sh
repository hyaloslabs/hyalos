#!/usr/bin/env bash
# Tries Monty's own type checker on the triage script against a stub of the no-notify SDK (Slice 4,
# S-05.3, of docs/plans/hyalos-kernel-python).
#
#   spikes/438/monty/typecheck.sh
#
# The stub, no-notify.pyi, is typed by hand from spikes/438/sdk/no-notify.py: it has `read_inputs`,
# `ask.classify` and `finish`, and no `notify`. The checker is Monty's `monty-type-checking` at the
# pinned commit (fetch.sh), which embeds `ty`, driven by typecheck/, a small host program that hands
# it the stub as module `hyalos`, as Monty's sessions hand it host module stubs. The tool's
# Cargo.lock is Monty's own, pruned by Cargo, and checked by `lock_matches` (fetch.sh).
#
# Prints one line, `typecheck: rejected` (the checker reports something, with its diagnostics
# after), `typecheck: accepted` (it reports nothing) or `typecheck: unusable: <why>` (it could not
# check the script, including the tool not building), and says whether README.md records the same
# result. It exits 0 in every case: S-05.3 records the result whatever it is.
set -uo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
readme="$root/spikes/438/README.md"

unusable() {
  echo "typecheck: unusable: $*"
  exit 0
}
fail() { unusable "$*"; }

# shellcheck source=fetch.sh
. "$here/fetch.sh"
work="$cache/monty-typecheck"

lock_matches "$here/typecheck/Cargo.lock" >/dev/null 2>&1 \
  || unusable "the tool's lockfile is not Monty's"
rm -rf "$work"
mkdir -p "$work"
git -C "$src" archive "$SHA" | tar -x -C "$work" || unusable "cannot copy Monty's tree"
cp -R "$here/typecheck" "$work/hyalos-typecheck"
log="$work/build.log"
(cd "$work/hyalos-typecheck" \
  && RUSTUP_TOOLCHAIN="$TOOLCHAIN" cargo build --release --locked >"$log" 2>&1) \
  || unusable "the tool did not build: $(grep -m 1 '^error' "$log" || tail -n 1 "$log")"

out=$("$work/hyalos-typecheck/target/release/hyalos-monty-typecheck" \
  "$here/no-notify.pyi" "$root/spikes/438/scripts/triage.py" 2>&1) \
  || unusable "the checker ended abnormally: $(printf '%s' "$out" | head -n 1)"
result=$(printf '%s\n' "$out" | head -n 1)
case "$result" in
  rejected | accepted | unusable:*) ;;
  *) unusable "the checker printed '$result'" ;;
esac
echo "typecheck: $result"
printf '%s\n' "$out" | tail -n +2
recorded=$(sed -n 's/^typecheck: \([a-z]*\).*/\1/p' "$readme" | head -n 1)
if [ "$recorded" = "${result%%:*}" ]; then
  echo "README.md records the same result"
else
  echo "README.md records '${recorded:-nothing}': update its typecheck: line"
fi
exit 0
