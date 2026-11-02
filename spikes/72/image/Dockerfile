FROM ubuntu:24.04
# Build-time network is allowed; the run-time container has none.
RUN apt-get update && apt-get install -y --no-install-recommends socat curl ca-certificates git iproute2 \
 && rm -rf /var/lib/apt/lists/*
COPY entry.sh /usr/local/bin/entry.sh
COPY hyalos-submit /usr/local/bin/hyalos-submit
RUN chmod 0755 /usr/local/bin/entry.sh /usr/local/bin/hyalos-submit \
 && (id agent >/dev/null 2>&1 || useradd -m -u 1001 agent || true)
ENTRYPOINT ["/usr/local/bin/entry.sh"]
