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

1. Open every link in all five files and confirm the target exists. The owner in `config.yml` was
   guessed once (`andreas-reichel`) and corrected to `twohreichel` — re-verify rather than trust it.
2. Enable GitHub Discussions in repository settings, or remove the Discussions contact link. A dead
   link on the issue chooser is worse than no link.
3. Enable private vulnerability reporting in repository settings, otherwise the security advisory link
   404s for outside contributors.
4. Render-check the issue forms: push to a branch and open the "New issue" chooser. A malformed form
   silently falls back to a blank issue, which is exactly the failure mode this ticket exists to
   prevent.
5. Verify the relative links resolve from the rendered view — `../CONTRIBUTING.md` and `../../discussions`
   are relative to `.github/`. There must be NO link into `.vscode/` — that tree is ignored, so the
   link is dead for every contributor. `docs/` is tracked and may be linked, but AC-14 still forbids
   the contribution surface from citing the specification or an `AC-n` id at all: it has to stand on
   its own. The current `pull_request_template.md` cites AC-1 and must be reworded here.
6. Verify `.github/FUNDING.yml` (AC-63): GitHub Sponsors must be ENABLED for the account or the
   button never appears — the key validates, the account is not checked. Every uncommented platform
   must resolve to a live page. Remove what does not, rather than leaving a dead link.
7. Verify the README carries both the purpose AND the non-goals that `CONTRIBUTING.md` delegates to
   it (AC-66). A contribution guide pointing at a scope statement that does not exist is worse than no
   pointer.
8. Add `SECURITY.md` if step 3 leaves anything unstated. **Ask first** — it was not in the original
   scope and this project ships cryptography, so its content is a real decision, not boilerplate.

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

# the contribution surface stands on its own: no spec reference, no AC-n id (AC-14)
grep -rnE 'AC-[0-9]+|specs?/spec\.md' CONTRIBUTING.md README.md .github/ && echo "FAIL: cites the spec" \
  || echo "ok: contribution surface is self-contained"

just format && just lint && just test && just qa
```

## Out of scope / Guardrails

- **No issue-tracker or internal-wiki references.** This repository is public and every reader has to
  be able to follow every link in it. That adaptation is already made — do not reintroduce them.
- **No CLA bot, no DCO check, no label automation.** The licence acknowledgement is a checkbox.
- Do not add a code of conduct without deciding on an enforcement contact — an unenforceable one is
  worse than none.

## Definition of Done

- [ ] AC-14 covered: template, both issue forms and `CONTRIBUTING.md` present and rendering
- [ ] AC-63 covered: sponsor button visible, or every funding line removed as unresolvable
- [ ] AC-66 covered: README states purpose and non-goals, CONTRIBUTING's pointer resolves
- [ ] Every link opened and confirmed, no guessed owner or path remaining
- [ ] Discussions and private vulnerability reporting either enabled or their links removed
- [ ] All verification commands pass, output pristine
- [ ] Committed as Conventional Commits, per `CONTRIBUTING.md`
