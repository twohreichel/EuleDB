# Backlog — EULEDB

Folder = status. WIP limit 1. This tree is tracked, so a status change is a `git mv` and lands in the
ticket's own commit — the backlog moves with the work rather than beside it.

    docs/backlog/
    ├── EULEDB-SUB-<n>.md   # session-sized ticket — ready & future
    ├── EULEDB-P<n>.md      # phase stub — NOT executable, must be cut first
    ├── in-progress/        # AT MOST ONE
    └── done/

## Two kinds of file, and the difference matters

- **`EULEDB-SUB-<n>.md`** — one session, one merge, size `S` to `L`. These are worked directly.
- **`EULEDB-P<n>.md`** — a whole phase, `size: epic`. **Never start one.** When it becomes next, cut it
  into `SUB` tickets of size S to L and work those. The phase files exist so no
  acceptance criterion is invisible: every `AC-n` in the spec is named by exactly one file here.

Ready = every id in `depends_on` is a file in `done/`.

## Session loop
/ticket-next  ->  mv the file  ->  /clear  ->  /ticket-work <id>  ->  /ticket-finish <id>  ->  /clear

## Rules
1. One ticket per session, `/clear` between tickets.
2. Read ONLY the files listed under the ticket's Context.
3. Plan first, wait for approval, then implement.
4. Verification commands are executable and must pass before done.
5. Guardrails are hard limits.
6. Escalate after two failed verification attempts.

## Source of truth

Spec: `docs/specs/spec.md`. Decision record: `docs/adr/`.

There is no `tasks.md` by decision — the cut lives in these ticket files. Each carries `fulfils` and
`depends_on` in its frontmatter, so coverage is checked over the frontmatter rather than in a separate
table:

```bash
python3 - <<'PY'
import re, glob, pathlib
spec = pathlib.Path("docs/specs/spec.md").read_text()
want = set(int(m) for m in re.findall(r'^- \*\*AC-(\d+):\*\*', spec, re.M))
have = set()
for f in glob.glob("docs/backlog/**/EULEDB-*.md", recursive=True):
    fm = re.search(r'^fulfils: (.*)$', pathlib.Path(f).read_text(), re.M)
    if fm: have |= set(int(x) for x in re.findall(r'AC-(\d+)', fm.group(1)))
print("criteria without a ticket:", sorted(want - have) or "none")
print("ticket criteria not in spec:", sorted(have - want) or "none")
PY
```
