# Contributing to EuleDB

Thank you for wanting to help. This document is short on purpose — it names the few rules that are
load-bearing and nothing else.

## Before you write code

**Open an issue first for anything non-trivial**, and let it be triaged. This avoids the worst outcome
for everyone: a finished pull request that has to be declined on scope.

EuleDB has a deliberately narrow remit — it is an embedded, local-first hybrid search engine, not a
server, not a general-purpose SQL engine, and not a cloud service. The README states what it is for. If
you are unsure whether an idea fits, [open a discussion](../../discussions) rather than a pull request.

Small changes need no ceremony. A typo, a broken link, a clearer sentence — send the pull request
directly.

## Tests come first, and that is not negotiable

Write the failing test, run it, watch it fail for the right reason, then write the smallest code that
makes it pass. A test written after the implementation tends to encode whatever the code happens to do,
including its bugs.

Specifically:

- Never weaken, skip or delete a test to reach green. Fix the code.
- Never assert only `is_some()` or "does not panic". State a behaviour that could fail.
- Derive expected values independently — a test that recomputes the production formula proves nothing.
- Mock only at system boundaries. Do not mock EuleDB's own types.

## The quality gate

One command, and all of it must pass before you open a pull request:

```bash
just format && just lint && just test && just qa
```

Behind those targets: `cargo fmt`, `cargo clippy --all-targets --all-features -D warnings`,
`cargo nextest run`, `cargo deny check`. CI runs the same thing on Linux x86_64, Linux aarch64, macOS arm64
and Windows x86_64, against both the pinned MSRV and stable. There is no way to merge past a red gate.

`unsafe` is forbidden at crate roots. If you genuinely need it in an index hot path, it goes in an
inventoried module with its invariant written down.

## Commit messages decide the version number

This repository generates its version and `CHANGELOG.md` from commit messages with Release Please, so
the format is functional rather than decorative:

    <type>(<scope>): <description>

- Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`.
- Header in the imperative, lower case, no trailing period, at most 72 characters.
- A breaking change carries `!` before the colon — `feat(storage)!: …` — or a `BREAKING CHANGE:`
  footer. While the version is below 1.0.0 that bumps the minor version.
- `feat` and `fix` appear in the changelog. `ci`, `test` and `chore` are hidden from it.

Never edit `CHANGELOG.md` or a version number by hand. Release Please owns both.

## Pull requests

The template asks for the self-review you would otherwise ask a reviewer to do for you. The two items
that matter most:

- **Keep it under ~400 changed lines**, ideally under 200. Defect detection drops sharply on large
  diffs, so a big change is not reviewed more thoroughly, it is reviewed worse. Split it.
- **Read your own diff first** and comment on anything non-obvious. Every line should trace to the
  change you set out to make — no drive-by reformatting, no unrelated "improvements" to code you
  happened to pass.

Review comments are about the code, never about the person, and a question is usually more useful than
a verdict.

## Security

Never open a public issue for a vulnerability. Use
[private reporting](../../security/advisories/new).

## Licence

Contributions are licensed under `Apache-2.0 OR MIT`, the same dual licence as the project. By opening
a pull request you confirm you have the right to contribute the code under those terms.
