---
id: EULEDB-SUB-5
ticket: EULEDB
fulfils: [AC-9]
depends_on: [EULEDB-SUB-4]
size: M
context_budget: 3000
safety: CI only
detail: full
status: done
pr: https://github.com/twohreichel/EuleDB/pull/5
---

## Goal

Harden the workflows and make the hardening mechanical. Every third-party action pinned to a full
40-character commit SHA with a version comment, `permissions: {}` at workflow level with least-privilege
grants per job, no `pull_request_target` executing fork code, no `${{ }}` interpolation of untrusted
event data into a `run:` block, `timeout-minutes` on every job, `persist-credentials: false` on every
checkout that does not push — and `actionlint` plus `zizmor` enforcing all of it so the next workflow
author cannot forget.

## Context (read ONLY these files)

- `.github/workflows/ci.yml`, `.github/workflows/supply-chain.yml`
- `justfile`, `CONTRIBUTING.md`
- `docs/specs/spec.md` (AC-9)

## The properties were already there. This ticket adds the enforcement.

SUB-3 and SUB-4 wrote both workflows to these rules deliberately, rather than writing them loosely and
fixing them here — shipping a known-vulnerable workflow and repairing it two tickets later is still
shipping it. So the audit found nothing, which is the intended outcome and not a reason to skip the
tooling: **the value is that the next workflow cannot quietly drop a property.**

Verified before the job was written, at the strictest setting, offline and online:

```
actionlint 1.7.12          -> exit 0, no findings
zizmor 1.29.0 --persona=auditor -> "No findings to report."
```

## Steps

1. A `workflow-lint` job in the CI workflow, wired into the `gate` job's `needs` so a finding blocks a
   merge like any other failure.
2. **`actionlint` is downloaded and checksum-verified, not added as a third-party action.** A job whose
   purpose is supply-chain hygiene should not widen the supply chain to do its work.
   `taiki-e/install-action` does not carry actionlint, and the alternatives are all additional actions,
   so the release archive is fetched and checked against the digest its own `checksums.txt` publishes:

   | Tool | Version | Digest source |
   |---|---|---|
   | `actionlint` | 1.7.12 | `8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8` |
   | `zizmor` | 1.29.0 | pinned version via `taiki-e/install-action` |

3. `zizmor` runs with `--persona=auditor`, the thorough setting, and with the workflow's own read-only
   token so the online audits run too. The one that matters there detects a pinned SHA that no longer
   exists in the action's repository — precisely the failure mode SHA-pinning is meant to make visible.
4. A `just lint-workflows` recipe, deliberately **not** part of `just lint`: it needs two binaries that
   have nothing to do with Rust, and failing a Rust contributor's gate with "command not found" would be
   worse than useless. `CONTRIBUTING.md` says when to run it and links both tools.

## The trade-off in `--persona=auditor`

The auditor persona reports informational findings, so a future `zizmor` release adding an audit can turn
a green gate red without the repository changing. That is accepted for the same reason it is accepted
from clippy: a new lint is information, and the alternative is a gate that only catches what was already
known when it was written.

## Verification (executable)

```bash
just lint-workflows                    # actionlint clean, zizmor clean at auditor persona
GH_TOKEN=$(gh auth token) zizmor --persona=auditor .github/workflows/   # online audits too

# no unpinned action anywhere, which is the property actionlint and zizmor now enforce
grep -nE '^\s+(- )?uses:' .github/workflows/*.yml \
  | grep -vE '@[0-9a-f]{40} # v' && echo "FAIL: unpinned" || echo "ok: all pinned"

# every job carries a timeout and a permissions block
python3 - <<'PY'
import yaml, glob
for f in glob.glob(".github/workflows/*.yml"):
    for name, job in yaml.safe_load(open(f))["jobs"].items():
        assert "timeout-minutes" in job, f"{f}:{name} has no timeout"
        assert "permissions" in job, f"{f}:{name} has no permissions block"
print("ok: every job is time-bounded and least-privileged")
PY

just format && just lint && just test && just qa
```

## Out of scope / Guardrails

- **No Dependabot configuration** — SUB-6, which is also where an action bump gets reviewed as the
  source diff between two SHAs rather than on the version comment.
- **No release workflow** — SUB-7.
- **No `pull_request_target`, ever.** On a public repository it hands fork code a privileged token. If
  something appears to need it, the requirement is wrong.
- Do not replace the checksum-verified download with an action to save six lines. The six lines are the
  point.

## Definition of Done

- [x] AC-9 covered: every property present in both workflows AND enforced by tooling in the gate
- [x] `actionlint` and `zizmor` run in the pipeline and block a merge on a finding
- [x] actionlint installed by digest, not by trusting a tag
- [x] `just lint-workflows` documented in `CONTRIBUTING.md` with both links verified
- [x] Commits follow Conventional Commits, grouped by concern
