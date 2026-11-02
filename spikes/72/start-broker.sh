#!/bin/bash
# Restart the stand-in broker. Keys are read from the repo's .env into the broker's own
# environment only; the sandbox never sees them.
#   PASSTHROUGH=both (default)  forward to the real APIs with the real keys
#   PASSTHROUGH=none            scripted replies, no model, no keys, no spend
#   ALLOW_HOSTS=a.com,b.com     hosts the proxy lets through (default example.com)
cd "$(dirname "$0")"
ROOT="$(cd ../.. && pwd)"
pkill -f '[s]tub-broker\.mjs'; sleep 0.4; rm -f /tmp/s72/broker.sock; rm -rf run; mkdir -p run; mkdir -p /tmp/s72; chmod 700 /tmp/s72   # the socket sits in a private directory: the broker holds real keys
MODE="${PASSTHROUGH:-both}"
( if [ "$MODE" != none ]; then set -a; . "$ROOT/.env"; set +a; fi
  PASSTHROUGH="$MODE" ALLOW_HOSTS="${ALLOW_HOSTS:-example.com}" nohup node stub-broker.mjs "$PWD/run" /tmp/s72/broker.sock > run/stub.out 2>&1 & )
sleep 1.5; cat run/stub.out
