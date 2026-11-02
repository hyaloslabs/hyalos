#!/bin/bash
# Shows which hostnames a harness tries to look up when it is allowed to try.
#   sinkhole.sh up                 start an internal Docker network (no external route) and a logging DNS sink
#   sinkhole.sh names <timestamp>  list the names queried since a UTC timestamp, most asked first
#                                  (take it with: date -u +%Y-%m-%dT%H:%M:%S)
#   sinkhole.sh down               remove both
# Run a harness on it with:  NETARGS="--network spike72net --dns 172.29.72.2" ./run-claude.sh
cd "$(dirname "$0")"
case "$1" in
  up)
    docker build -q -t hyalos-spike72-sink -f image/sink.Dockerfile image >/dev/null
    docker network rm spike72net >/dev/null 2>&1
    docker network create --internal --subnet 172.29.72.0/24 spike72net >/dev/null
    docker rm -f s72sink >/dev/null 2>&1
    docker run -d --name s72sink --network spike72net --ip 172.29.72.2 hyalos-spike72-sink \
      dnsmasq --no-daemon --log-queries --log-facility=- --no-resolv --no-hosts \
      --address=/#/172.29.72.2 --listen-address=172.29.72.2 --bind-interfaces >/dev/null
    sleep 1; echo "sinkhole up on spike72net (172.29.72.2)" ;;
  names)
    ts="${2:?usage: sinkhole.sh names <UTC timestamp, e.g. 2026-11-02T03:00:00>}"
    case "$ts" in *Z|*+*) ;; *) ts="${ts}Z" ;; esac   # docker reads a bare timestamp as local time
    docker logs --since "$ts" s72sink 2>&1 \
      | grep -oE 'query\[[A-Z]+\] [^ ]+' | awk '{print $2}' | sort | uniq -c | sort -rn ;;
  down)
    docker rm -f s72sink >/dev/null 2>&1; docker network rm spike72net >/dev/null 2>&1; echo "sinkhole removed" ;;
  *) echo "usage: sinkhole.sh up|names <timestamp>|down"; exit 2 ;;
esac
