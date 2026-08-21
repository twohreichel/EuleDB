---
id: EULEDB-SUB-3
ticket: EULEDB
fulfils: [AC-7, AC-11]
depends_on: [EULEDB-SUB-2]
size: L
context_budget: 3000
safety: CI only, no runtime code
detail: full
status: done
pr: https://github.com/twohreichel/EuleDB/pull/3
---

## Goal

The quality pipeline. Formatting, lint, tests and the documentation build as separately reported jobs,
matrixed over linux-x86_64, linux-aarch64, macOS-arm64 and windows-x86_64, against the pinned MSRV and
against stable. A platform the matrix does not cover must not be claimed as supported anywhere.

## Context (read ONLY these files)

- `.github/workflows/ci.yml` (new)
- `justfile` — the pipeline runs these commands, not its own
- `rust-toolchain.toml` — pins the MSRV leg
- `CONTRIBUTING.md` — states which platforms are claimed
- `docs/specs/spec.md` (AC-7, AC-11)

## Steps

1. Four jobs, so each reports separately and can be required individually: `format`, `clippy`, `test`,
   `doc`. Each runs the same command the corresponding `just` recipe runs.
2. A fifth `gate` job that depends on all four and fails unless every one succeeded. Branch protection
   requires this, so the rule "no merge while anything fails" survives a matrix that grows — a
   hand-maintained list of matrix job names does not.
3. Matrix: the four claimed platforms times the two toolchains, 8 test legs, `fail-fast: false` so one
   platform failing still reports the others.
4. `clippy` runs on one platform but both toolchains. Clippy gains and changes lints between releases
   far more often than a target triple changes what it reports, and there is no platform-specific code.
5. No toolchain installation step on the MSRV legs: `rust-toolchain.toml` pins the channel and lists
   `rustfmt` and `clippy`, so rustup installs both on the first cargo call. The stable legs install
   stable and set `RUSTUP_TOOLCHAIN`, which overrides the pin.
6. A test asserting the matrix and the claimed-platform list agree in both directions — the mechanical
   half of AC-11, which is otherwise a promise nobody checks.

## Hardening applied here, not deferred

AC-9 belongs to SUB-5, but writing a workflow that violates it and fixing it two tickets later would
be shipping a known defect. So this workflow already carries: every third-party action pinned to a full
40-character commit SHA with a version comment, `permissions: {}` at workflow level with `contents: read`
granted per job, `persist-credentials: false` on every checkout, `timeout-minutes` on every job, and
matrix values passed through `env:` rather than interpolated into a `run:` block.

Both SHAs were resolved from the registry and confirmed to be real commits, not guessed:

| Action | Version | Commit |
|---|---|---|
| `actions/checkout` | v7.0.1 | `3d3c42e5aac5ba805825da76410c181273ba90b1` |
| `taiki-e/install-action` | v2.86.4 | `a2a5f6e99e1a31540baa0468acfa302cff0f359f` |

SUB-5 adds `actionlint` and `zizmor` so these properties are enforced mechanically instead of by
review, which is the part that genuinely belongs there.

## Verification (executable)

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))" && echo "ok: parses"

# every third-party action is pinned to a 40-character SHA, never to a tag
grep -nE '^\s+(- )?uses:' .github/workflows/*.yml \
  | grep -vE '@[0-9a-f]{40} # v' && echo "FAIL: an action is not SHA-pinned" || echo "ok: all pinned"

# the matrix and the claimed platforms agree, in both directions
cargo nextest run -E 'test(test_matrix)'

just format && just lint && just test && just qa
```

Then the part only the live pipeline can prove. The first run was green on all 13 jobs — 8 test legs
across the four platforms and both toolchains, plus format, both clippy legs, doc and gate. Branch
protection on `main` now requires all 13 by the names that run produced, with `enforce_admins` on and
linear history required, which matches the rebase-merge policy.

## Out of scope / Guardrails

- **No `cargo deny`, no `cargo audit`, no weekly schedule** — SUB-4 owns the supply-chain gates.
- **No `actionlint`, no `zizmor`** — SUB-5.
- **No dependency caching.** It would save nothing on a 16-package tree. It becomes worth its moving
  parts and its cache-poisoning surface when the Lance dependency lands, so it is noted on SUB-10
  rather than added speculatively here.
- **No release or publish workflow** — SUB-7.
- Do not widen the matrix. A platform added here becomes a support promise, and the specification is
  explicit that the honest fix for a platform that costs too much is to stop claiming it.

## Definition of Done

- [x] AC-7 covered: format, clippy, test and doc report separately and all are required
- [x] AC-11 covered: 4 platforms times 2 toolchains, all green, and no undocumented platform
- [x] The live run is green before the merge, on every leg
- [x] Required status checks configured from the job names the run produced
- [x] Commits follow Conventional Commits, grouped by concern
