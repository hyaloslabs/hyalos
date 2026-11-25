from hyalos import task

# The issue-triage task, against the generated SDK. It keeps every rule of the digest guest
# (hyalos-os/guest/src/lib.rs), so on the same issues and answers its notification and result are
# the digest guest's bytes.

MAX_RECORDS = 200
TITLE_MAX = 4096
DIGEST_MAX = 8000
TRUNC_LINE = b"...truncated\n"

counts = {"bug": 0, "feature": 0, "security": 0, "other": 0}
security = []
total = 0

for issue in task.read_inputs()[:MAX_RECORDS]:
    total += 1
    # Too large to read, no tab, or a number that is not strict decimal: other, without asking.
    if issue.number is None or issue.title is None:
        counts["other"] += 1
        continue
    title = issue.title[:TITLE_MAX]
    label = task.ask.classify(title)
    if label == "security":
        security.append((issue.number, title))
    counts[label] += 1

digest = (
    b"total " + str(total).encode()
    + b" bug " + str(counts["bug"]).encode()
    + b" feature " + str(counts["feature"]).encode()
    + b" security " + str(counts["security"]).encode()
    + b" other " + str(counts["other"]).encode()
    + b"\n"
)

for k in range(len(security)):
    number, title = security[k]
    line = b"#" + str(number).encode() + b" " + title + b"\n"
    last = k + 1 == len(security)
    # Keep room for the truncation line unless nothing follows this line.
    if len(digest) + len(line) + len(TRUNC_LINE) <= DIGEST_MAX or (
        last and len(digest) + len(line) <= DIGEST_MAX
    ):
        digest += line
    else:
        digest += TRUNC_LINE
        break

task.notify.team(digest)
task.finish(b"ok " + str(total).encode())
