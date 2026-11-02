#!/bin/bash
# Usage: run-codex.sh [extra codex -c args...]   (prompt via $PROMPT)
cd "$(dirname "$0")"
CX=$HOME/.npm-global/lib/node_modules/@openai/codex/node_modules/@openai/codex-linux-x64/vendor/x86_64-unknown-linux-musl
PROMPT="${PROMPT:-Run hyalos-submit with a short summary, then stop.}"
docker run --rm ${NETARGS:---network none} --user 1000:1000 --read-only --tmpfs /tmp --tmpfs /home/ubuntu:uid=1000,gid=1000 \
  -v /tmp/s72/broker.sock:/run/broker.sock -v "$CX":/opt/codex:ro \
  -e HOME=/home/ubuntu -e CODEX_HOME=/home/ubuntu/.codex -e HYALOS_KEY=placeholder-not-a-key ${EXTRA_DOCKER:-} \
  -e PATH=/opt/codex/path:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
  -w /tmp hyalos-spike72 bash -c 'mkdir -p "$CODEX_HOME" && exec timeout "${TMO:-90}" /opt/codex/codex/codex exec --skip-git-repo-check --sandbox "${SANDBOX:-danger-full-access}" "$@"' _ \
  -c model_provider=hyalos -c 'model_providers.hyalos={name="hyalos", base_url="http://127.0.0.1:8089/v1", env_key="HYALOS_KEY", wire_api="responses"}' -c model=stub-model \
  "$@" "$PROMPT" 2>&1
