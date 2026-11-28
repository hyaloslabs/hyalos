#!/usr/bin/env bash
# The operator's run for Slice 3 (criteria 3.1 and 3.2). An executor never runs this: it reads the
# operator's model credential, writes to the public log and posts one public issue comment.
#
#   bash attest/probes/two_machines.sh <ssh host> <owner/repo> <issue number>
#
# Run from hyalos-os. It starts a notary on the other machine under keys kept there,
# reached through an SSH tunnel, and stops it when this script ends.
set -euo pipefail
[ $# -eq 3 ] || { echo "usage: two_machines.sh <ssh host> <owner/repo> <issue number>"; exit 2; }
host=$1 repo=$2 issue=$3
port=7047 remote=.hyalos-notary out=evidence/out/two-machines
attest=$PWD/target/release/attest
monitor=$PWD/target/release/task-kernel
key=$HOME/.config/hyalos-spike/operator-transport.key
# A live run on a real repository needs the KVM kernel (S-04.5 of docs/plans/hyalos-kernel-core), so
# the kernel image is built from this tree, as the monitor's tests build it, before anything starts.
kernel=$PWD/kernel-image/target/x86_64-unknown-none/release/hyalos-kernel
(cd kernel-image && cargo build --release --locked) || { echo "the kernel image did not build (if the target is missing: rustup target add x86_64-unknown-none)"; exit 2; }
[ -x "$attest" ] && [ -x "$monitor" ] && [ -x target/release/notary ] && [ -x target/release/canary ] && [ -s "$kernel" ] || { echo "build both programs and the kernel image first"; exit 2; }
[ ! -e "$out" ] || { echo "$out already exists: move it away first"; exit 2; }

operator_public=$("$attest" link-key "$key")
ssh "$host" "mkdir -p -m 700 $remote"
scp -q target/release/notary "$host:$remote/notary"
scp -q target/release/canary "$host:$remote/canary"

# The customer runs `canary check` on that machine with their own credential. This script never
# reads it. Without a fresh pass it stops here, before the notary starts or anything is published.
ssh "$host" "$remote/canary status --dir $remote/state" >/dev/null || {
    echo "no fresh customer canary pass on $host: the customer must run canary check there first"
    exit 2
}
mkdir -p "$out"

coproc NOTARY { ssh -o ExitOnForwardFailure=yes -L "$port:127.0.0.1:$port" "$host" \
    "$remote/notary --listen 127.0.0.1:$port --dir $remote/state --operator-transport-public $operator_public --stop-on-stdin-close"; }
notary_pid=$NOTARY_PID
trap 'kill "$notary_pid" 2>/dev/null || true' EXIT
read -r -t 30 -u "${NOTARY[0]}" _ signer
read -r -t 5 -u "${NOTARY[0]}" _ transport
read -r -t 5 -u "${NOTARY[0]}" _ _listening
echo "notary signing public key: $signer"

# The customer's side authorises this task for two runs: the timing call and the real task. A state
# directory from an earlier probe already holds a round for it, so that case takes a new round.
# Every command reads its inputs framed, so the fixture is framed first (ADR-0138).
"$monitor" frame "$out/issues.inputs" --lines evidence/fixtures/issues.tsv > /dev/null
task=$("$monitor" task guest/digest.wasm "$out/issues.inputs")
authorize="$remote/notary authorize --dir $remote/state --task $task --attempts 2"
authorisation=$(ssh "$host" "$authorize 2>/dev/null || $authorize --again")

export HYALOS_ATTEST=$attest HYALOS_NOTARY_ADDRESS=127.0.0.1:$port
export HYALOS_NOTARY_TRANSPORT_PUBLIC=$transport HYALOS_NOTARY_SIGNER_PUBLIC=$signer
export HYALOS_NOTARY_AUTHORISATION=$authorisation
export HYALOS_OPERATOR_TRANSPORT_KEY=$key
# The guest runs inside the KVM machine: without a kernel, `live` refuses a real repository.
export HYALOS_KERNEL=$kernel

# 3.2: the round trip, as the median of 20 TCP connection times to the other machine's SSH port.
address=$(ssh -G "$host" | awk '$1=="hostname"{print $2}')
ssh_port=$(ssh -G "$host" | awk '$1=="port"{print $2}')
python3 -I - "$address" "$ssh_port" <<'PY'
import socket, statistics, sys, time
times = []
for _ in range(20):
    start = time.perf_counter()
    socket.create_connection((sys.argv[1], int(sys.argv[2])), timeout=5).close()
    times.append((time.perf_counter() - start) * 1000)
print(f"round trip: median of 20 connections {statistics.median(times):.1f} ms")
PY

# 3.2: one attested call under a run of its own, so it spends no number of the real run.
timing_run=$(head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n')
printf 'The build fails on a clean checkout' > "$out/question.txt"
/usr/bin/time -f 'one attested call: %e s wall, %M KB peak (operator side)' \
    "$attest" ask "$out/timing.presentation" "$timing_run" 0 < "$out/question.txt" | sed 's/"response_id":"[^"]*"/"response_id":"…"/'

# 3.1: one real task through the notary. This posts one comment on the issue named.
"$monitor" live guest/digest.wasm "$out/issues.inputs" "$out/record.json" "$repo" "$issue"
printf '%s\n' "$signer" > "$out/notary.pub"
"$monitor" attested "$out/record.json" --notary-public "$out/notary.pub"
