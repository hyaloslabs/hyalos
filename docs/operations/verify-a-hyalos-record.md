# Verify a Hyalos record

This guide is for a customer who wants to check a Hyalos run without trusting the operator who ran it. You need a Linux x86-64 machine. The verifier runs no other program, writes no file and reads nothing under your home directory.

**Do not take the verifier from the operator.** A verifier the operator hands you proves nothing, because the operator could have changed it. Get it from the repository's releases and confirm it yourself, as below.

## 1. Get the verifier

Find the newest `verifier-v*` release at <https://github.com/hyaloslabs/hyalos/releases>, then download it with GitHub's CLI:

```
tag=verifier-v0.1.0
gh release download "$tag" --repo hyaloslabs/hyalos --pattern hyalos-verify-x86_64-linux --pattern SHA256SUMS
sha256sum -c SHA256SUMS
chmod +x hyalos-verify-x86_64-linux
```

If no verifier release exists yet, there is nothing to download or attest, so skip steps 2 and 3 and build it from the commit you were asked to trust, with Docker:

```
git clone https://github.com/hyaloslabs/hyalos
cd hyalos
git checkout <commit>
.github/scripts/build-verifier.sh dist
```

The program is `dist/hyalos-verify-x86_64-linux`. Use that path in step 5.

## 2. Confirm it came from this repository's release workflow

This needs `gh` 2.68 or later and uses no Hyalos tool:

```
gh attestation verify hyalos-verify-x86_64-linux --repo hyaloslabs/hyalos --signer-workflow hyaloslabs/hyalos/.github/workflows/release-verifier.yml --source-ref "refs/tags/$tag"
```

A copy whose bytes differ from the attested build, by even one byte, fails this check. Do not use a copy that fails it.

## 3. Rebuild it, if you do not want to trust GitHub

This check trusts no build service. In any directory, with Docker:

```
git clone --branch "$tag" https://github.com/hyaloslabs/hyalos
cd hyalos
.github/scripts/build-verifier.sh dist
cat dist/SHA256SUMS
```

The hash must equal the one in the release's `SHA256SUMS`. The rebuilt program is `dist/hyalos-verify-x86_64-linux`, so use that path in step 5 if you run your own build. The build uses a Rust image named by digest and a pinned C compiler, so the directory you build in does not change the result.

`hyalos-verify-x86_64-linux --version` prints the version, the source commit and the record versions it reads.

## 4. Gather the evidence and the keys

| You need | Where it comes from |
|---|---|
| The record, with `<record>.salt` and `<record>.attest/` beside it | The operator |
| The directory of links, `NNNNN.json` from `00000` | The operator |
| The guest program | The operator. The record fixes its hash |
| `notary.pub`, the notary's public key | **You.** Use the key of the notary you chose and run, never one the operator gives you |
| `operator.pub`, the operator's public key | **A separate channel**, such as a contract or a call, and not the same delivery as the evidence |

A pass holds only under the keys you give it. If the operator chose either key for you, a pass tells you nothing about them.

## 5. Run it

```
./hyalos-verify-x86_64-linux record.json links --operator-public operator.pub --notary-public notary.pub --guest guest.wasm
```

A version 1 record needs `--legacy-v1` and no `--guest`.

It asks the public log which entries it holds under the operator's key, so it needs network access to `rekor.sigstore.dev`.

| Exit | Means | What to do |
|---|---|---|
| 0 | Passed. The report is on standard output | Read the limits below |
| 1 | Refused. A check failed on readable evidence, and standard error names the first | Treat the record as not proven |
| 2 | Inconclusive. Something could not be checked, such as an unreachable log | Fix the cause and run it again. This is neither a pass nor evidence of forgery |

## 6. What a pass does not prove

The report prints the lines that apply to your record. Each is a limit you accept when you rely on a pass.

Always printed:

- `not proven: the notary's independence is the customer's own`
- `not proven: the model's name is the provider's claim`
- `not proven: the answer's correctness is not judged`
- `not proven: the log is trusted to have shown every entry under the operator's key`
- `not proven: the request's hidden bytes are not shown, so the call may have used another credential or carried an added header`

Printed for records of version 2 and later:

- `not proven: the operator may have asked the provider directly, away from the notary`
- `not proven: the customer did not authorise this task again after this attestation`

Printed for records of version 3 and later:

- `not proven: the canary tested one forged shape, with the customer's credential, at the time stated`

Printed for version 2 records only:

- `not proven: this record has no provider canary`

The format, the options and the size limits are in [`hyalos-os/RECORD.md`](../../hyalos-os/RECORD.md).
