# Unsafe code inventory

**The inventory is empty. EuleDB contains no `unsafe` code.**

Every crate root declares `#![forbid(unsafe_code)]`, which the compiler enforces and
`crates/euledb/tests/repository_invariants.rs` re-checks for any crate added later, so this file
cannot silently fall out of date.

## When it stops being empty

There is exactly one case in which `unsafe` is admissible: a measured hot path in an index
implementation where a safe formulation has been shown to cost real performance. Not a suspected cost
— a measured one, with the benchmark in the repository.

Adding it requires all four of these, in the same pull request:

1. The `unsafe` lives in **one named module**, not scattered across a crate. That module downgrades
   the crate-level `forbid` to `#![deny(unsafe_code)]` with `#[allow(unsafe_code)]` on the module, so
   the exception stays local and visible.
2. **This file gains a row** naming the module, the invariant every block relies on, and why the
   safe version was not good enough. An entry that says "for performance" without a number is not an
   entry.
3. **Every block carries a `// SAFETY:` comment** stating which invariant makes it sound and who
   guarantees it. A block whose invariant cannot be written down in a sentence is not understood
   well enough to be written.
4. **The benchmark that motivated it** is in the repository and runnable, so the next person can
   check whether the trade-off still holds on their hardware. Hardware changes. Reasons expire.

| Module | Invariant | Why safe Rust was not enough |
|---|---|---|
| — | — | — |

## Why this file is at the repository root

A safety inventory is read by people deciding whether they can depend on this crate — an auditor, a
reviewer, someone in a regulated environment. It belongs where they look first, not one directory in.
