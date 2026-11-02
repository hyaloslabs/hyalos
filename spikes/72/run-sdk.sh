#!/bin/bash
# Usage: [NETARGS=...] [DISABLE=0|1] run-sdk.sh <claude|openai>
cd "$(dirname "$0")"
SDK="$PWD/sdk"
NODE=$(readlink -f "$(command -v node)")
OFF=()
if [ "${DISABLE:-1}" = 1 ]; then
  OFF=(-e CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 -e DISABLE_AUTOUPDATER=1 -e DISABLE_TELEMETRY=1 -e DISABLE_ERROR_REPORTING=1 -e OPENAI_AGENTS_DISABLE_TRACING=1)
fi
case "$1" in claude) SCRIPT=claude-sdk.mjs ;; openai) SCRIPT=openai-sdk.mjs ;; *) echo "claude|openai"; exit 2 ;; esac
docker run --rm ${NETARGS:---network none} --user 1000:1000 --read-only --tmpfs /tmp --tmpfs /home/ubuntu:uid=1000,gid=1000 \
  -v /tmp/s72/broker.sock:/run/broker.sock -v "$NODE":/usr/local/bin/node:ro -v "$SDK":/opt/sdk:ro \
  -e HOME=/home/ubuntu -e ANTHROPIC_BASE_URL=http://127.0.0.1:8089 -e ANTHROPIC_API_KEY=placeholder-not-a-key \
  -e OPENAI_BASE_URL=http://127.0.0.1:8089/v1 -e OPENAI_API_KEY=placeholder-not-a-key "${OFF[@]}" ${EXTRA_DOCKER:-} \
  -w /opt/sdk hyalos-spike72 timeout "${TMO:-90}" node "$SCRIPT" 2>&1
