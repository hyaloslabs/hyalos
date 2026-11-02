#!/bin/bash
# Usage: [NETARGS=...] [DISABLE=0|1] run-claude.sh [extra claude args...]
cd "$(dirname "$0")"
CL=${CLAUDE_BIN:-$(readlink -f ~/.local/bin/claude)}
[ -f "$CL" ] || { echo "Claude Code binary not found at $CL: set CLAUDE_BIN" >&2; exit 1; }
PROMPT="${PROMPT:-Run hyalos-submit with a short summary, then stop.}"
OFF=()
if [ "${DISABLE:-1}" = 1 ]; then
  OFF=(-e CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 -e DISABLE_AUTOUPDATER=1 -e DISABLE_TELEMETRY=1 -e DISABLE_ERROR_REPORTING=1)
fi
docker run --rm ${NETARGS:---network none} --user 1000:1000 --read-only --tmpfs /tmp --tmpfs /home/ubuntu:uid=1000,gid=1000 \
  -v /tmp/s72/broker.sock:/run/broker.sock -v "$CL":/usr/local/bin/claude:ro \
  -e HOME=/home/ubuntu -e ANTHROPIC_BASE_URL=http://127.0.0.1:8089 -e ANTHROPIC_API_KEY=placeholder-not-a-key "${OFF[@]}" ${EXTRA_DOCKER:-} \
  -w /tmp hyalos-spike72 timeout "${TMO:-90}" claude -p "$PROMPT" --allowedTools "${ALLOWED:-Bash(hyalos-submit:*)}" "$@" 2>&1
