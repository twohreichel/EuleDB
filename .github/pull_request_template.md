## Description

- **What** does this pull request change?
- **Why** is the change needed?
- **How** does it work — the approach, and what you considered instead?

## References

- Closes: #<issue>
- Related pull requests or discussions, if any.

## Change type

- [ ] Bug fix
- [ ] New or removed feature
- [ ] Refactor — no behaviour change
- [ ] Tests
- [ ] Documentation
- [ ] Build, CI or dependencies

## Author self-review

Tick these before requesting review. They are the same checks a reviewer would otherwise spend their
time on.

- [ ] **Focused and small** — under ~400 changed lines, ideally under 200. A larger change is split.
- [ ] **Self-reviewed** — I read my own diff and annotated anything non-obvious with a PR comment.
- [ ] **Gate green locally** — `just format && just lint && just test && just qa`
- [ ] **Test written before the code**, observed failing first, and it would fail again on a real bug.
      No test was weakened or skipped to get to green.
- [ ] No debug prints, commented-out code, or leftover TODOs.
- [ ] Docs, doc-comments and public API documentation updated where the change touched them.
- [ ] No secret, key, token or credential in the diff or in its history.
- [ ] `unsafe` was not introduced — or, if it was, it lives in one named module, is listed in
      [`UNSAFE.md`](https://github.com/twohreichel/EuleDB/blob/main/UNSAFE.md) with its invariant, and every block carries a `// SAFETY:` comment.

## Commit messages

This repository derives its version number and `CHANGELOG.md` from commit messages via Release Please,
so the format is load-bearing, not cosmetic:

    <type>(<scope>): <description>

`feat` / `fix` / `docs` / `refactor` / `perf` / `test` / `build` / `ci` / `chore`. A breaking change
carries `!` after the scope or a `BREAKING CHANGE:` footer. See
[CONTRIBUTING.md](https://github.com/twohreichel/EuleDB/blob/main/CONTRIBUTING.md).

## Licence

- [ ] I agree that my contribution is licensed under `Apache-2.0 OR MIT`, matching the project.
