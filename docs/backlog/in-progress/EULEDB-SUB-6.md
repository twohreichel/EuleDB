---
id: EULEDB-SUB-6
ticket: EULEDB
fulfils: [AC-10]
depends_on: [EULEDB-SUB-5]
size: S
context_budget: 3000
safety: config only
detail: full
status: in-progress
---

## Goal

Dependabot for the `cargo` and `github-actions` ecosystems, weekly, patch and minor grouped, majors
ungrouped, a cooldown before a fresh release is proposed, no blanket `ignore`, and no auto-merge for
actions.

## Context (read ONLY these files)

- `.github/dependabot.yml` (new)
- `scripts/check-dependabot.py` (new)
- `.github/workflows/ci.yml` — the policy check joins the `workflow-lint` job
- `justfile`
- `docs/specs/spec.md` (AC-10)

## Steps

1. Both ecosystems, weekly on Monday morning UTC. Weekly rather than daily because a daily cadence on a
   single-maintainer project produces a queue nobody reads, and a queue nobody reads is how a security
   update gets merged late.
2. Cooldown: 3 days for a patch, 5 for a minor, 14 for a major. A version yanked or patched within days
   of publication is common enough that reviewing it immediately is mostly wasted effort, and a major
   gets the longest wait because its regressions surface last.
3. Patch and minor grouped into one pull request per ecosystem — individually they are noise, together
   they are one review of one lockfile diff. **Majors deliberately ungrouped**: each is a breaking change
   that deserves its own pull request, its own CI run and its own decision.
4. `commit-message.prefix` is `build` for cargo and `ci` for actions, with the scope included, so
   Release Please files a bump under "Build and dependencies" instead of into Features.
5. The three labels the config applies (`dependencies`, `rust`, `github-actions`) created in the
   repository first. Dependabot fails the update when a referenced label does not exist.
6. `scripts/check-dependabot.py` guarding the policy, in `workflow-lint` and in `just lint-workflows`.

## Two absences that are the point

**No `ignore` entry anywhere.** `ignore` suppresses security updates for the matched dependency as well
as version updates, so a blanket ignore is a way of not being told about a vulnerability. Holding a
specific version back belongs in the manifest as a constraint with a comment, where it is visible.

**Nothing is auto-merged, and no workflow does it.** Actions least of all: SHA-pinning buys immutability
against a retag attack and pays for it by needing these pull requests, so an action bump has to be
reviewed as the source diff between the two commits. Merging on the strength of the version comment in
the bump hands back exactly what the pinning bought.

## Why a script and not the published schema

`scripts/check-dependabot.py` checks the *policy*, not the syntax. GitHub already validates the syntax
and reports a broken file in the dependency graph — where nobody looks until an update stops arriving.
What nothing else checks is whether both ecosystems are still covered, whether an `ignore` has crept in,
whether the cooldown is still there, and whether a group has started sweeping majors up with patches.

Validating against the published JSON schema would be the better *syntax* check, and it was used once
during authoring to confirm `cooldown` and `groups` are accepted. It is deliberately not in the gate: it
would make the pipeline depend on a third-party URL at runtime, and a gate that fails when someone
else's CDN is down is a gate people learn to ignore.

## Verification (executable)

```bash
python3 scripts/check-dependabot.py     # ok on both lines
just lint-workflows                     # actionlint, zizmor, and the policy check

# every check in the script has to be able to fail — verified by breaking each one:
#   an `ignore:` entry           -> "It suppresses security updates as well as version updates"
#   `major` added to a group     -> "Each breaking change deserves its own pull request"
#   an ecosystem renamed         -> "no update entry for the github-actions ecosystem"

# the labels the config applies must exist, or the update fails
gh label list --json name -q '.[].name' | grep -E '^(dependencies|rust|github-actions)$'

just format && just lint && just test && just qa
```

## Out of scope / Guardrails

- **No auto-merge workflow.** Not for cargo, and emphatically not for actions.
- **No `ignore` entry**, now or later, for the reason above.
- **No Release Please configuration** — SUB-7.
- Do not raise `open-pull-requests-limit` to make the queue move faster. A queue that is too long is a
  signal about review capacity, not about the limit.

## Definition of Done

- [ ] AC-10 covered: both ecosystems, weekly, grouped patch and minor, ungrouped majors, cooldown set
- [ ] No `ignore` entry and no auto-merge anywhere
- [ ] The policy check runs in the pipeline and locally, and every one of its checks observed failing
- [ ] The three referenced labels exist in the repository
- [ ] Commits follow Conventional Commits, grouped by concern
