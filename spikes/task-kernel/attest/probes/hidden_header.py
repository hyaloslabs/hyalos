#!/usr/bin/env python3
"""Issue #396: does the provider answer a body forged inside the request's hidden bytes?

Run by the operator: `python3 -I attest/probes/hidden_header.py <credential file>`.

Verify pins the request's head, so its content-length is the approved body's. A forged body in
the hidden bytes would therefore be followed, up to that length, by the shown blank line and the
start of the approved body. This asks whether the provider answers such a request.

It makes two small calls (one output token each). It prints case names, status codes and a
verdict, and never the credential, its length, or anything from a reply's body.

The HYALOS_TRUST_* settings point it at the local stand-in, for testing the probe itself. Under
them it refuses a real-shaped credential, and without them it refuses any other kind.
"""
import os, socket, ssl, sys

HIDDEN = 106  # CREDENTIAL_LENGTH in attest/src/main.rs
END = "\r\n\r\n"
MODEL = "claude-haiku-5-5"
SMALLEST = '{"model":"%s","max_tokens":1,"messages":[{"role":"user","content":"x"}]}' % MODEL
APPROVED = '{"model":"%s","max_tokens":1,"messages":[{"role":"user","content":"A longer question that stands in for the approved request, so that its length is not the smuggled one"}]}' % MODEL


def credential_from(path):
    # Read as the attest tool reads it. Refused by a fixed message: an error could quote a byte.
    try:
        with open(path, encoding="ascii") as file:
            line = next((l for l in file if l.strip()), "").strip()
    except (OSError, UnicodeDecodeError):
        sys.exit("refused: cannot read the credential file")
    value = line.split("=", 1)[-1].strip().strip("\"'")
    if not value or not all(33 <= ord(c) <= 126 for c in value):
        sys.exit("refused: the file does not hold one credential")
    test = bool(os.environ.get("HYALOS_TRUST_ROOT"))
    if not value.startswith("test-" if test else "sk-ant-"):
        sys.exit("refused: the credential is not the kind these settings allow")
    return value


def status(request):
    root = os.environ.get("HYALOS_TRUST_ROOT")
    name = os.environ.get("HYALOS_TRUST_SERVER_NAME", "api.anthropic.com")
    host, port = os.environ.get("HYALOS_TRUST_PROVIDER_ADDRESS", name + ":443").rsplit(":", 1)
    # The stand-in's root is a DER file, as the attest tool reads it.
    context = ssl.create_default_context(cadata=open(root, "rb").read()) if root else ssl.create_default_context()
    try:
        with socket.create_connection((host, int(port)), timeout=60) as raw:
            with context.wrap_socket(raw, server_hostname=name) as tls:
                tls.sendall(request.encode())
                line = tls.makefile("rb").readline(64).split()
        return int(line[1])
    except (OSError, ValueError, IndexError):
        return 0  # no HTTP status: the connection failed or was cut


def head(length, credential):
    """The head the attest tool writes, up to and including the credential."""
    name = os.environ.get("HYALOS_TRUST_SERVER_NAME", "api.anthropic.com")
    auth = (f"x-api-key: {credential}" if credential.startswith("sk-ant-api")
            else f"anthropic-beta: oauth-2025-04-20\r\nauthorization: Bearer {credential}")
    return (f"POST /v1/messages HTTP/1.1\r\nhost: {name}\r\nanthropic-version: 2023-06-01\r\n"
            f"content-type: application/json\r\naccept-encoding: identity\r\nconnection: close\r\n"
            f"content-length: {length}\r\n{auth}")


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: hidden_header.py <credential file>")
    credential = credential_from(sys.argv[1])
    control = status(head(len(SMALLEST), credential) + END + SMALLEST)
    print(f"control, the small body alone: {control}")
    if control != 200:
        print("verdict: inconclusive, the control was not answered")
        return 2
    forged = status(head(len(APPROVED), credential) + END + SMALLEST + END + APPROVED)
    print(f"small body followed by the shown bytes: {forged}")
    fits = len(credential) + len(END) + len(SMALLEST) <= HIDDEN
    print(f"it would fit in the hidden bytes with this credential: {'yes' if fits else 'no'}")
    if 400 <= forged < 500:
        print("verdict: the provider refused the forged body")
        return 0
    if forged == 200:
        print("verdict: FORGERY WORKS" if fits else "verdict: the provider accepts the shape, and this credential leaves no room for it")
        return 1
    print("verdict: inconclusive, the forged request got no clear answer")
    return 2


if __name__ == "__main__":
    sys.exit(main())
