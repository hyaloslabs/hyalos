# Contributing to Hyalos

Thank you for helping. Hyalos is MIT licensed ([`LICENSE`](LICENSE)), and every contribution is made under that licence. The reasons for these terms are in [ADR-0115](docs/adr/0115-mit-licence-and-dco-sign-off.md).

## Sign off every commit

Hyalos uses the [Developer Certificate of Origin](https://developercertificate.org/) (DCO) 1.1, not a contributor licence agreement. You keep the copyright in your work. By signing off, you certify the statement below for that commit.

Add the sign-off with `git commit -s`. It appends a line with your name and the email address of your commit:

```
Signed-off-by: Your Name <you@example.com>
```

Use your real name. A pull request merges only when every commit in it is signed off. To sign off commits you have already made, run `git rebase --signoff main` and force-push your branch.

```
Developer Certificate of Origin
Version 1.1

Copyright (C) 2004, 2006 The Linux Foundation and its contributors.

Everyone is permitted to copy and distribute verbatim copies of this
license document, but changing it is not allowed.


Developer's Certificate of Origin 1.1

By making a contribution to this project, I certify that:

(a) The contribution was created in whole or in part by me and I
    have the right to submit it under the open source license
    indicated in the file; or

(b) The contribution is based upon previous work that, to the best
    of my knowledge, is covered under an appropriate open source
    license and I have the right under that license to submit that
    work with modifications, whether created in whole or in part
    by me, under the same open source license (unless I am
    permitted to submit under a different license), as indicated
    in the file; or

(c) The contribution was provided directly to me by some other
    person who certified (a), (b) or (c) and I have not modified
    it.

(d) I understand and agree that this project and the contribution
    are public and that a record of the contribution (including all
    personal information I submit with it, including my sign-off) is
    maintained indefinitely and may be redistributed consistent with
    this project or the open source license(s) involved.
```

## Before you open a pull request

- Read [`CLAUDE.md`](CLAUDE.md). It holds the rules for the code, and they apply to people and agents alike. For the design, start at [`docs/adr/README.md`](docs/adr/README.md).
- Run what CI runs. The commands are listed under "Before pushing" in `CLAUDE.md`.
- Never commit a secret. Test keys are made for the tests alone and are listed in [`.gitleaks.toml`](.gitleaks.toml), which states why each one is harmless. To scan the full history, run:

  ```
  gitleaks git --log-opts="--all" --redact .
  ```

- Changes under `core/` (the trusted core) need code-owner review (`.github/CODEOWNERS`).

## Reporting a vulnerability

Do not open a public issue for a vulnerability. Follow [`SECURITY.md`](SECURITY.md).
