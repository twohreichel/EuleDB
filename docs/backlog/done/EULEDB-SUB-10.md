---
id: EULEDB-SUB-10
ticket: EULEDB
fulfils: [AC-16, AC-17]
depends_on: [EULEDB-SUB-9]
size: L
context_budget: 3000
safety: trait boundary keeps the on-disk format swappable
detail: full
status: done
pr: https://github.com/twohreichel/EuleDB/pull/10
---

## Goal

Rows persist to disk and come back byte-identical after the handle is dropped and reopened, reached only
through an internal trait so no module outside the storage crate names a type from the format — which is
pinned to an exact version.

## Context (read ONLY these files)

- `docs/adr/ADR-001-lance-as-storage-format.md` — read first
- `crates/euledb-storage/src/store.rs`, `crates/euledb-storage/src/lib.rs`
- `crates/euledb-storage/tests/persistence.rs`
- `crates/euledb/tests/repository_invariants.rs`
- `Cargo.toml`, `deny.toml`, `.cargo/audit.toml`, `justfile`
- `docs/specs/spec.md` (AC-16, AC-17)

## Four things this ticket discovered that no amount of planning would have

**1 — The format dictates the Arrow version, and SUB-9 got it wrong.** `lance` 10.0.0 depends on Arrow
**58**, while SUB-9 declared 59. Two Arrow majors in one tree are two incompatible type worlds: a
`RecordBatch` built against 59 cannot be handed to a `lance` built against 58, and it surfaces as a type
error rather than a warning. Arrow is now pinned to 58, and the exact pin on the format is what keeps
that alignment from drifting under a patch release.

The lesson is the prior-art gate, not the version: the peer's manifest should have been read before
choosing a version, and it was not.

**2 — Default features pull in every cloud object store there is.** `lance` enables `aws`, `azure`,
`gcp`, `oss`, `tencent`, `tos`, `goosefs`, `huggingface` and `geo` by default. For a database whose
entire premise is that nothing leaves the machine, and whose ceiling is 50 MB resident while idle,
linking a fleet of cloud SDKs and an HTTP stack in would contradict both the non-goals and AC-4.
`default-features = false`.

**3 — `protoc` is a build prerequisite, and the alternative is worse.** The format generates code from
protobuf at build time. Its own `protoc` feature builds the compiler from source instead — but that
needs `cmake` and a C++ toolchain and rebuilds protobuf on every clean build across eight CI legs.
Requiring the small prebuilt binary and installing a pinned one in CI is the cheaper trade. Documented
in `CONTRIBUTING.md` with per-platform install commands, because a missing prerequisite is only a
friendly error if the reader is told where to get it.

**4 — The dependency tree went from 18 crates to 477.** That is ADR-001's accepted cost, and it is worth
stating in a number rather than as "a large tree". Cold build is roughly two minutes.

## What the supply-chain gate did with all that

`multiple-versions = "deny"` was set strict in SUB-4 specifically so this moment would be a decision
rather than a discovery. It found 13 duplicates, of which **four were the Arrow ones** — the real
problem, surfaced by a policy that exists to surface exactly that. Aligning Arrow resolved them.

The remaining eight are transitive duplicates inside the format's tree, none reachable from a manifest
here. They are listed individually in `deny.toml` with a reason each, **not waved away by loosening the
rule**: a duplicate outside that list still fails the build. Verified by commenting one entry out.

Two licences and one advisory then needed real decisions:

| Finding | Decision | Reason |
|---|---|---|
| `xxhash-rust` is BSL-1.0 | added to the allow-list | Boost is permissive, OSI-approved, no copyleft. Its absence was an oversight in the "permissive class", not a policy |
| `option-ext` is MPL-2.0 | **one named exception**, MPL stays off the list | file-level copyleft does not constrain the larger work's terms, the source stays on the registry, and it arrives via `dirs` under `lance-index`. A *direct* MPL dependency would still stop the build |
| `paste` is unmaintained (RUSTSEC-2024-0436) | ignored by id, in both tools | true and not a vulnerability: a proc macro its author considers finished, build-time only, transitive. A real advisory against it would be a different id |

Each of the three was verified to be load-bearing by removing it and watching the gate fail.

## Design

`TableStore` is a **driven port**, not an abstraction over variants — it exists to invert the dependency
on the format, so one implementation is the expected number rather than a sign of a missing second. Its
methods are `async` because the format's API is: a library that calls `block_on` internally panics when
its caller is already inside a runtime, which is a defect rather than a convenience.

`StorageError` keeps the cause as an opaque boxed error rather than a typed variant. A
`#[from] <format>::Error` variant would put the format's type in this crate's public API, and then every
caller would depend on the thing the trait boundary exists to contain. The chain stays reachable through
`Error::source`.

**`append` does not validate against the stored schema yet.** The format rejects a mismatching append
itself, so nothing corrupt is written — what is lost is the named message AC-15 produces. Composing the
two belongs where insert becomes public API, and doing it here would mean converting the format's schema
back to an Arrow schema for no caller that exists yet.

## TDD record

1. **rows survive a drop and reopen** → RED: `no LanceStore in the root`, `no TableStore in the root`.
2. Mutation check on the green implementation: writing nothing → caught. Scanning nothing → caught.
   **Append silently replaced by overwrite → NOT caught.** A real gap: nothing guarded that a second
   append preserves the first, which in a database is data loss with no test to notice.
3. **a second append adds to the first** → written to close that gap, and confirmed to fail under the
   overwrite mutation.
4. **no crate outside the storage layer names the format** → the mechanical half of AC-17. Verified
   three ways: it fires on a manifest declaration naming file and line, it stays quiet inside the
   storage crate where the dependency is allowed, and it does not trip on `balance` or `glance` — a
   substring check would have, and a check with false positives gets switched off within a week.

The doc gate then caught a broken public doc link (a field documented as pointing at a private type
alias). Fixed by writing the explanation the link was standing in for.

## Verification (executable)

```bash
just format && just lint && just test && just qa   # protoc must be on PATH

# the boundary, mechanically
cargo nextest run -E 'test(on_disk_format)'

# the format is pinned exactly, and brings no cloud backend
grep -n 'lance = ' Cargo.toml     # ="=10.0.0", default-features = false

# every deny.toml decision is load-bearing — remove one, the gate fails
#   MPL exception removed      -> licenses FAILED
#   advisory ignore removed    -> advisories FAILED
#   BSL-1.0 removed from allow -> licenses FAILED
#   a skip entry removed       -> bans FAILED
```

## Out of scope / Guardrails

- **No compression settings, no encryption** — SUB-11 and SUB-12.
- **No update, no delete, no drop** — SUB-15 and later.
- **No public re-export from the facade crate.** The boundary test would fail if there were, which is
  the point.
- **Never add a cloud feature to the format dependency.** Not for convenience, not for a test. The
  non-goals are explicit and the resident-memory ceiling is not negotiable.
- Do not resolve a future duplicate by loosening `multiple-versions`. One entry, one reason.

## Definition of Done

- [x] AC-16 covered: rows come back byte-identical after a drop and reopen, and a second append adds
- [x] AC-17 covered: trait boundary in place, format pinned exactly, and a test proves nothing outside
      the storage crate names it
- [x] Every test observed failing first, and every branch shown to be guarded by a mutation check
- [x] Arrow aligned to the version the format requires, with the reason recorded
- [x] Every supply-chain decision recorded with a reason and shown to be load-bearing
- [x] `protoc` documented as a prerequisite and installed in every job that builds
- [x] Commits follow Conventional Commits, grouped by concern
