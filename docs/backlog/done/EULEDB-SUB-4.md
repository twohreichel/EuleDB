---
id: EULEDB-SUB-4
ticket: EULEDB
fulfils: [AC-8]
depends_on: [EULEDB-SUB-3]
size: M
context_budget: 3000
safety: CI only
detail: full
status: done
pr: https://github.com/twohreichel/EuleDB/pull/4
---

## Goal

Supply-chain gates. `cargo deny check` over advisories, licences, bans and sources, plus `cargo audit`,
on every pull request AND on a weekly schedule — so an advisory published after a merge surfaces without
anyone touching the code. The licence check fails on any dependency licence incompatible with
`Apache-2.0 OR MIT`.

## Context (read ONLY these files)

- `deny.toml` (new)
- `.github/workflows/supply-chain.yml` (new)
- `justfile` — `qa` gains the supply-chain recipe
- `docs/specs/spec.md` (AC-8)

## What the licence list is

**A policy, not a prediction.** The permissive class only: `Apache-2.0`,
`Apache-2.0 WITH LLVM-exception`, `MIT`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Zlib`, `Unicode-3.0`.
Every copyleft licence is deliberately absent — MPL-2.0, LGPL, GPL, AGPL and relatives — because the
project ships under `Apache-2.0 OR MIT` and a dependency has to permit redistribution under both. A
dependency outside the list is a licensing decision that needs a written reason, which is exactly why
the build stops rather than warns.

The list therefore names licences the current 18-crate tree does not contain, so
`unused-allowed-license = "allow"` keeps the gate output pristine. Output nobody reads is output that
hides a real finding.

## Decisions in deny.toml worth knowing

- `yanked = "deny"`. A yanked version is one its author withdrew. Building against it anyway is a
  decision, so it has to be made rather than not noticed.
- `unmaintained = "all"`. An unmaintained dependency is the single-maintainer risk the specification
  names, arriving through the back door.
- `multiple-versions = "deny"`, with an empty `skip` list. Two versions of one crate mean duplicated
  compile time, attack surface and maintenance. Realistic while the tree is small — **revisit at SUB-10**,
  when Lance and Arrow land: a duplicate that genuinely cannot be resolved gets a `skip` entry with its
  reason and stays visible, rather than the policy being loosened wholesale before there is evidence.
- `unknown-git = "deny"`. A git dependency has no version, no yank mechanism and no advisory coverage.
- `targets` lists exactly the four supported platforms. A dependency only ever compiled for a target
  nobody supports is not part of this project's supply chain.

## Both tools, and the measurement that explains why

Measured on 2026-08-21 by adding an MPL-2.0 crate (`option-ext`) to the tree in two positions:

| Position | `cargo deny check licenses` | `cargo audit` |
|---|---|---|
| normal dependency | **rejects** | n/a, licences are not its job |
| dev-dependency | passes | covers it, it reads `Cargo.lock` and all 18 crates |

So `cargo-deny` evaluates normal and build dependencies, and ignoring dev-dependencies for the licence
check is **correct**: a dev-dependency is never redistributed, so its licence cannot constrain how this
crate may be licensed. But a dev-dependency does execute during `cargo test`, locally and in CI, which
makes it a real attack surface, and that half is covered by `cargo-audit` because `Cargo.lock` lists the
dev tree.

The two tools therefore cover different halves rather than the same ground twice. That is why AC-8 names
both, and why running only one would leave a genuine gap.

## Verification (executable)

```bash
cargo deny check                # advisories ok, bans ok, licenses ok, sources ok
cargo audit --deny warnings
just qa                         # doc, publish-check and supply-chain

python3 -c "import yaml; yaml.safe_load(open('.github/workflows/supply-chain.yml'))" && echo "ok: parses"

# still no unpinned action anywhere
grep -nE '^\s+(- )?uses:' .github/workflows/*.yml \
  | grep -vE '@[0-9a-f]{40} # v' && echo "FAIL: unpinned" || echo "ok: all pinned"

# The licence gate has to be able to FAIL, or it is decoration. Removing one licence from the
# allow-list does NOT prove that: every crate here is dual-licensed MIT OR Apache-2.0, so the OR
# stays satisfied by the other half. The real test is a crate with no allowed alternative.
printf '\n[dependencies]\noption-ext = "0.2"\n' >> crates/euledb-storage/Cargo.toml   # MPL-2.0
cargo deny check licenses       # MUST fail: "license is not explicitly allowed"
git checkout crates/euledb-storage/Cargo.toml Cargo.lock
```

## Out of scope / Guardrails

- **No `actionlint`, no `zizmor`** — SUB-5. The properties they enforce are already present in both
  workflows; what belongs there is the enforcement, so the next workflow author cannot forget them.
- **No Dependabot configuration** — SUB-6.
- **No auto-merge of anything.** A dependency bump is a review decision, and an action bump is reviewed
  as the source diff between two SHAs, never on the version comment alone.
- Do not add a licence to the allow-list to make a build pass. That is the decision the gate exists to
  force into the open.

## Definition of Done

- [x] AC-8 covered: both tools, on pull requests, on `main`, and weekly
- [x] The licence gate observed failing when a licence in the tree is removed from the allow-list
- [x] `just qa` runs both tools locally with pristine output
- [x] The new job names added to the required status checks after the first run
- [x] Commits follow Conventional Commits, grouped by concern
