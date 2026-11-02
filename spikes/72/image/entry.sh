#!/bin/bash
# The forwarder: a loopback port inside the sandbox relayed to the broker's channel.
# The channel is the only thing mounted in; the container has no network.
socat TCP-LISTEN:8089,bind=127.0.0.1,fork,reuseaddr UNIX-CONNECT:/run/broker.sock &
for i in $(seq 1 50); do (exec 3<>/dev/tcp/127.0.0.1/8089) 2>/dev/null && break; sleep 0.1; done
exec "$@"
