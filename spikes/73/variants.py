#!/usr/bin/env python3
"""Build the test inputs under run/: trust configs, a tampered artifact, and bundles with evidence removed.

Usage: variants.py [history]   (history needs a network: it downloads old trusted roots from Sigstore's TUF repo)
"""
import base64, copy, glob, json, os, shutil, sys, urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
RUN = os.path.join(HERE, "run")
VEC = os.path.join(HERE, "vectors")
CFG = "application/vnd.dev.sigstore.clienttrustconfig.v0.1+json"
SIGNING = {"mediaType": "application/vnd.dev.sigstore.signingconfig.v0.2+json"}


def write_cfg(root_path, out_path):
    root = json.load(open(root_path))
    json.dump({"mediaType": CFG, "trustedRoot": root, "signingConfig": SIGNING}, open(out_path, "w"))


def main():
    # Clear only what this script generates: run/rust holds the built verifier.
    for d in ("cfg", "tamper", "strip", "hist"):
        shutil.rmtree(os.path.join(RUN, d), ignore_errors=True)
        os.makedirs(os.path.join(RUN, d))
    write_cfg(glob.glob(os.path.join(VEC, "trusted_root.*.json"))[0], os.path.join(RUN, "cfg", "current.json"))

    # A tampered artifact: the checksums file with one line appended.
    src = os.path.join(VEC, "cosign-v3.1.4", "cosign_checksums.txt")
    open(os.path.join(RUN, "tamper", "cosign_checksums.txt"), "wb").write(open(src, "rb").read() + b"tampered\n")

    # Bundles with one piece of evidence removed from the fresh bundle.
    bundle = json.load(open(src + ".sigstore.json"))

    def variant(name, edit):
        b = copy.deepcopy(bundle)
        edit(b)
        json.dump(b, open(os.path.join(RUN, "strip", name + ".sigstore.json"), "w"))

    vm = lambda b: b["verificationMaterial"]
    variant("no-tlog-entries", lambda b: vm(b).update(tlogEntries=[]))
    variant("no-inclusion-proof", lambda b: vm(b)["tlogEntries"][0].pop("inclusionProof", None))
    variant("no-log-promise (SET)", lambda b: vm(b)["tlogEntries"][0].pop("inclusionPromise", None))
    variant("no-rfc3161-timestamp", lambda b: vm(b).pop("timestampVerificationData", None))

    def no_time(b):
        vm(b)["tlogEntries"][0].pop("inclusionPromise", None)
        vm(b).pop("timestampVerificationData", None)

    variant("no-log-promise-and-no-timestamp", no_time)

    def flip(b):
        sig = base64.b64decode(b["messageSignature"]["signature"])
        b["messageSignature"]["signature"] = base64.b64encode(bytes([sig[0] ^ 1]) + sig[1:]).decode()

    variant("flipped-signature", flip)

    if len(sys.argv) > 1 and sys.argv[1] == "history":
        base = "https://tuf-repo-cdn.sigstore.dev"
        get = lambda p: urllib.request.urlopen(base + p, timeout=20).read()
        # Current targets version, from the repository's own metadata: timestamp -> snapshot -> targets.
        snap_v = json.loads(get("/timestamp.json"))["signed"]["meta"]["snapshot.json"]["version"]
        n = json.loads(get(f"/{snap_v}.snapshot.json"))["signed"]["meta"]["targets.json"]["version"]
        seen = {}
        for v in range(n, 0, -1):
            try:
                t = json.loads(get(f"/{v}.targets.json"))["signed"]
            except Exception:
                continue
            tr = t["targets"].get("trusted_root.json")
            if not tr:
                continue
            h = tr["hashes"]["sha256"]
            seen.setdefault(h, [v, v, t["expires"][:10]])[1] = v
        for h, (hi, lo, exp) in sorted(seen.items(), key=lambda kv: -kv[1][0]):
            path = os.path.join(RUN, "hist", f"v{lo}-{hi}.root.json")
            open(path, "wb").write(get(f"/targets/{h}.trusted_root.json"))
            write_cfg(path, path.replace(".root.json", ".cfg.json"))
            print(f"trusted root targets v{lo}..v{hi} (metadata expiry {exp})")


main()
