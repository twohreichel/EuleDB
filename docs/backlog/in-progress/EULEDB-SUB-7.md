---
id: EULEDB-SUB-7
ticket: EULEDB
fulfils: [AC-12, AC-13]
depends_on: [EULEDB-SUB-6]
size: M
context_budget: 3000
safety: the release pull request is inert until merged
detail: full
status: in-progress
---

## Goal

Version and changelog derived from Conventional Commits by Release Please, never written by hand, and
publication only from a tag that a merged release pull request created — after the quality and
supply-chain gates passed on that exact commit.

## Context (read ONLY these files)

- `.github/workflows/release.yml` (new)
- `release-please-config.json`, `.release-please-manifest.json`
- `Cargo.toml` — the version baseline
- `docs/specs/spec.md` (AC-12, AC-13)

## The version baseline was wrong, and it mattered

The manifest claimed `0.1.0` as the last released version while nothing had ever been released, so the
first release Release Please proposed would have been **0.2.0**. Baseline set to `0.0.0` in both the
manifest and `[workspace.package]`, which makes the first release `0.1.0` — and makes the placeholder
publish that claims the registry name `0.0.0`, exactly as SUB-2 originally described it. The two tickets
now agree instead of contradicting each other.

`bootstrap-sha` was the empty string, which is not a commit. Removed: without it Release Please
considers the whole history, which is what the first changelog should cover.

## The default token cannot do this job

A pull request opened with `GITHUB_TOKEN` **does not trigger workflows.** Its required checks would
therefore never run, and branch protection would make the release pull request permanently unmergeable.
So the workflow needs a fine-grained personal access token (`contents: write`, `pull-requests: write`)
in the `release` environment.

Until that secret exists the workflow **annotates the run with a warning and skips**, rather than
failing. A permanently red workflow on the default branch teaches everyone to ignore it, which is worse
than a warning that says what is missing and why.

## Publishing uses trusted publishing, not a stored token

`rust-lang/crates-io-auth-action` exchanges a short-lived OIDC identity for a scoped registry token, so
no long-lived registry credential lives in this repository at all. It needs `id-token: write` and a
trusted publisher configured on crates.io — which can only be configured for a crate that already
exists, so it is blocked behind the placeholder publish.

## AC-13's ordering is proved, not assumed

Branch protection already makes it true by construction: the release pull request goes through the same
17 required checks as any other, so a tagged commit has passed the gates. The `verify-gates` job proves
it anyway, by reading the check runs of the released commit and refusing to publish unless every one
concluded `success`, `neutral` or `skipped`. "By construction" stops being true the moment somebody
relaxes a setting.

## Verification (executable)

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml'))" && echo "ok: parses"
just lint-workflows        # actionlint clean, zizmor clean at the auditor persona

# the baseline that decides the first version number
python3 -c "import json; print(json.load(open('.release-please-manifest.json')))"   # {'.': '0.0.0'}
grep '^version' Cargo.toml                                                          # 0.0.0

# the environments the workflow references must exist
gh api repos/twohreichel/EuleDB/environments -q '.environments[].name'              # release, crates-io

just format && just lint && just test && just qa
```

The end-to-end proof cannot be run here: it needs the token, and then a merge to the default branch to
open the first release pull request. Both are recorded below as maintainer steps.

## Blocked on the maintainer

1. **Create the `RELEASE_PLEASE_TOKEN`** — a fine-grained personal access token scoped to this
   repository with `contents: write` and `pull-requests: write`, stored in the `release` environment.
   Nothing releases until this exists.
2. **Publish `0.0.0` once, by hand**, to claim the registry name. This is the only irreversible step in
   the whole chain, and an agent must not hold registry credentials.
3. **Configure the trusted publisher on crates.io** for `euledb` and `euledb-storage`, pointing at this
   repository and the `release.yml` workflow. Only possible after step 2, because a trusted publisher is
   configured on an existing crate.

## Out of scope / Guardrails

- **Never hand-edit `CHANGELOG.md` or a version number.** Release Please owns both, and a manual edit
  makes the next run disagree with the repository.
- **No publish outside this workflow.** The registry token is a short-lived OIDC exchange, so there is
  nothing to copy out and use locally, and that is the point.
- **No `--allow-dirty` in the publish step**, unlike the local gate: this runs from a clean tag
  checkout, so the check is the strict one.
- Do not add the release workflow's jobs to the required status checks. It runs on `push` to the default
  branch, not on pull requests, so it has nothing to gate.

## Definition of Done

- [ ] AC-12 covered: version and changelog derived from Conventional Commits, no hand-written version
- [ ] AC-13 covered: publish only from a release tag, and only after the gates passed on that commit
- [ ] The version baseline makes the first release 0.1.0 rather than 0.2.0
- [ ] Workflow clean under actionlint and zizmor at the auditor persona
- [ ] The three maintainer steps recorded where they will be found again
- [ ] Commits follow Conventional Commits, grouped by concern
