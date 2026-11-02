FROM ubuntu:24.04
# A DNS sinkhole: answers every name with its own address and logs each query.
RUN apt-get update && apt-get install -y --no-install-recommends dnsmasq && rm -rf /var/lib/apt/lists/*
