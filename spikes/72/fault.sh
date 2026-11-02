#!/bin/bash
# Usage: fault.sh <claude|codex> <label> <fault-json>
# Sets the broker's fault, runs one harness, and reports how it reacted.
cd "$(dirname "$0")"
who=$1; label=$2; fault=$3
: > run/broker.log; echo "$fault" > run/fault.json
export PROMPT='Run: hyalos-submit "{\"summary\":\"fault test\"}" and stop.'
export EXTRA_DOCKER="-e TMO=45"; export TMO=45
s=$(date +%s.%N)
if [ "$who" = claude ]; then out=$(ALLOWED="Bash" ./run-claude.sh --model claude-haiku-5-5 --tools "Bash"); else out=$(./run-codex.sh -c model=gpt-5-mini -c model_reasoning_effort=low -c analytics.enabled=false -c web_search=disabled); fi
code=$?; e=$(date +%s.%N)
rm -f run/fault.json
faults=$(grep -c '"fault"' run/broker.log); ok=$(grep -c '"upstream":"' run/broker.log); sub=$(grep -c '"submit"' run/broker.log)
printf '%-8s %-34s %5.1fs | injected errors seen: %-2s | live model calls ok: %s | task submitted: %s\n' "$who" "$label" "$(echo "$e - $s" | bc)" "$faults" "$ok" "$([ "$sub" -gt 0 ] && echo yes || echo no)"
echo "$out" | grep -v -E "^(OpenAI Codex|--------|workdir|model:|provider|approval|sandbox|reasoning|session id|mcp startup|user$|tokens used)" | grep -v '^$' | tail -3 | cut -c1-220 | sed 's/^/           /'
