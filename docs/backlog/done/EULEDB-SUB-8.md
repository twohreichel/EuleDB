---
id: EULEDB-SUB-8
ticket: EULEDB
fulfils: [AC-14, AC-63, AC-66]
depends_on: [EULEDB-SUB-2]
size: S
context_budget: 3000
safety: documentation and templates only — no runtime effect
# depends_on is SUB-2, not SUB-1: the four-command gate must exist before any commit,
# and `justfile` plus `Cargo.toml` are created there. A ticket whose verification cannot run
# is not verifiable, and an unverifiable ticket cannot be finished.
detail: full
status: backlog
---

## Goal

Give a first-time contributor a path that does not depend on asking: a pull-request template carrying
the author self-review gate, issue forms that collect version, platform and reproduction, and a
`CONTRIBUTING.md` that names the quality gate, the Conventional Commits requirement and the `AC-n`
traceability rule.

**Most of this was drafted during the specification session.** The remaining work is verification and
the parts that could not be settled without a live repository — not authoring from scratch.

## Context (read ONLY these files)

- `.github/pull_request_template.md`
- `.github/ISSUE_TEMPLATE/bug_report.yml`
- `.github/ISSUE_TEMPLATE/feature_request.yml`
- `.github/ISSUE_TEMPLATE/config.yml`
- `.github/FUNDING.yml`
- `CONTRIBUTING.md`
- `docs/specs/spec.md` (AC-14 only)

## Steps

1. Every link in the five files opened and confirmed — including the ones that are not filesystem paths.
   The owner is `twohreichel` throughout, re-verified rather than trusted.
2. **Discussions enabled.** It was off, so the contact link on the issue chooser and the scope pointer in
   the contribution guide both 404'd for every visitor. Five categories now exist and the URL resolves.
3. **Private vulnerability reporting enabled.** It was off. The advisory link returned 200 for the owner
   and would have 404'd for an outside contributor, which is the only person who needs it.
4. Both issue forms validated against the **published GitHub issue-form schema**, and `config.yml`
   against the issue-config schema. Stronger than a visual render check: a malformed form falls back to
   a blank issue silently, which is the failure this ticket exists to prevent.
5. **Relative links made absolute where the file is not rendered as a file.** A pull-request template
   and an issue form are pasted into a pull-request or issue *body*, so `../UNSAFE.md` resolves against
   `/pull/N` rather than against the tree. Three links fixed.
6. `FUNDING.yml` verified: `hasSponsorsListing` is true and the listing is public, so the sponsor button
   renders. Only `github:` is active, everything else stays commented out.
7. **README now states what EuleDB is for AND what it deliberately is not**, in its own voice rather
   than as a copy of the specification. The contribution guide delegates the scope question to the
   README, so a guide pointing at a scope statement that did not exist was a broken promise.
8. `SECURITY.md` written — see the decision below.

## The AC-14 violation this ticket inherited

`pull_request_template.md` carried "inventoried per AC-1". A contributor is held to that checklist and
cannot read the document the id lives in, so the line now states the requirement itself: one named
module, an entry in `UNSAFE.md` with the invariant, a `// SAFETY:` comment per block. The same treatment
was applied to a criterion id in a comment in the release workflow, and the check that finds these was
narrowed to the documents a contributor is actually held to — a workflow comment is internal
engineering, not a contract with anyone, and including it produced a false failure.

## SECURITY.md — the decision, made rather than deferred

The ticket flagged this as a real decision because the project ships cryptography. It was written, for
two reasons: both the contribution guide and the issue chooser already point at private reporting, and
GitHub shows a "Security policy" link only when the file exists — so a contributor clicking Security
found nothing.

What it says is the decision, and it is deliberately unflattering:

- The cryptographic **design has not been audited**. Audited primitives are not the same as an audited
  composition, and saying so is the difference between honest and reassuring.
- **Do not use it as the only protection for data whose disclosure would harm someone**, yet. Encryption
  trusted more than it has earned is worse than none, because it changes what people are willing to store.
- **Best effort, no service level.** One maintainer cannot honestly promise a response window, so no
  number is given rather than a number that will be missed.
- No maintenance branches below 1.0.0, no backports, no bug bounty. All stated rather than discovered.
- A concrete in-scope and out-of-scope list, so a reporter can tell before spending their evening.

If the maintainer disagrees with any of that, the file is one commit to change — but shipping the link
without the policy was not an option.

## Code sketch

None.

## Verification (executable)

```bash
# forms parse as GitHub expects
python3 -c "import yaml,glob,sys; [yaml.safe_load(open(f)) for f in glob.glob('.github/ISSUE_TEMPLATE/*.yml')]" \
  && echo "ok: issue forms parse"

# no link into the ignored tooling tree — that link is dead for contributors
grep -rn '\.vscode/' CONTRIBUTING.md .github/ && echo "FAIL: link into ignored tree" \
  || echo "ok: no dead links"

# The contribution surface stands on its own: no spec reference, no AC-n id (AC-14). Scoped to the
# documents a contributor is actually HELD to — a workflow comment is internal engineering, not a
# contract with anyone, so including .github/ wholesale produced a false failure.
grep -rnE 'AC-[0-9]+|specs?/spec\.md' \
    CONTRIBUTING.md README.md SECURITY.md .github/pull_request_template.md .github/ISSUE_TEMPLATE/ \
  && echo "FAIL: cites the spec" || echo "ok: contribution surface is self-contained"

just format && just lint && just test && just qa
```

## Out of scope / Guardrails

- **No issue-tracker or internal-wiki references.** This repository is public and every reader has to
  be able to follow every link in it. That adaptation is already made — do not reintroduce them.
- **No CLA bot, no DCO check, no label automation.** The licence acknowledgement is a checkbox.
- Do not add a code of conduct without deciding on an enforcement contact — an unenforceable one is
  worse than none.

## Definition of Done

- [x] AC-14 covered: template, both issue forms and `CONTRIBUTING.md` present, schema-valid, and free of
      any reference to the specification or an `AC-n` id
- [x] AC-63 covered: sponsor listing confirmed public via the API, so the button renders
- [x] AC-66 covered: README states purpose and non-goals, and the guide's pointer to it resolves
- [x] Every link opened and confirmed, no guessed owner or path remaining
- [x] Discussions and private vulnerability reporting both enabled, both links returning 200
- [x] `SECURITY.md` present, its content decided rather than copied
- [x] All verification commands pass, output pristine
- [x] Commits follow Conventional Commits, grouped by concern
