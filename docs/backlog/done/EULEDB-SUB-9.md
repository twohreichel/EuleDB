---
id: EULEDB-SUB-9
ticket: EULEDB
fulfils: [AC-15]
depends_on: [EULEDB-SUB-7]
size: M
context_budget: 3000
safety: new crate content, no consumer yet
detail: full
status: done
pr: https://github.com/twohreichel/EuleDB/pull/9
---

## Goal

A table schema IS an Apache Arrow schema, and an insert whose record batch does not match the
declaration is rejected — naming the offending column and what was wrong with it.

## Context (read ONLY these files)

- `crates/euledb-storage/src/schema.rs`, `crates/euledb-storage/src/lib.rs`
- `crates/euledb-storage/tests/schema.rs`
- `Cargo.toml`, `deny.toml`
- `docs/specs/spec.md` (AC-15) and its § Glossary

## Design

`TableSchema` wraps an `arrow_schema::SchemaRef`. There is deliberately **no second, private notion of
a type**: the same description serves storage, the query path and anything the caller already uses
Arrow for, which is the whole reason Arrow is the interchange contract.

`validate(&RecordBatch) -> Result<(), SchemaMismatch>` is a pure query. It touches no disk, allocates
only the error it returns, and can therefore be called before a write is attempted rather than after
one half-succeeded.

Four ways a batch can be wrong, each its own variant carrying the column name:

| Variant | Fires when |
|---|---|
| `MissingColumn` | the declaration names a column the batch does not have |
| `UndeclaredColumn` | the batch carries a column the declaration does not name |
| `TypeMismatch` | the types differ, and the error carries **both** |
| `NullabilityMismatch` | the batch permits null where the declaration forbids it |

Two decisions inside that are not obvious:

- **Columns match by name, not by position.** A caller assembling columns from a map has no control
  over their order, and refusing a correct batch over ordering would be pedantry.
- **A batch may be stricter than the declaration, never looser.** Forbidding null where the
  declaration permits it is fine — every value satisfies the declaration. The reverse would make the
  declaration a suggestion.

**One mismatch is reported, not all of them.** AC-15 says "naming the offending column and the
mismatch", singular, and a `Vec<SchemaMismatch>` is a larger API commitment than the criterion asks
for. Recorded here because it is the kind of thing a caller with three wrong columns will want changed,
and then it is a deliberate change rather than a discovery.

## Dependencies added

`arrow-schema` and `arrow-array` at 59, plus `thiserror` 2. **Not the `arrow` facade**: this layer needs
the schema types and `RecordBatch`, not the compute kernels, the CSV reader or the FFI layer. A consumer
using the facade gets the same types, because the facade re-exports these.

`thiserror` here rather than a hand-written `Display`: the error type is the public surface of every
failure this crate can produce, and the derive keeps the message next to the variant it describes.

### The first real test of the duplicate-version policy

`multiple-versions = "deny"` fired immediately: `syn` appears twice, 2.0.119 through `zerocopy-derive`
under `arrow-array`, and 3.0.3 through `thiserror-impl`. **Both paths are proc-macro only.** Proc macros
run at build time and their code is not linked into the artifact, so the cost is compile time and
nothing else — and neither version is ours to pick.

Resolved exactly as SUB-4 said it should be: a `skip` entry for that one duplicate, with the reason
written down. Not by loosening the policy.

## TDD record

Six vertical slices, each with the failing test observed first:

1. accept a batch matching the declaration → RED: `no TableSchema in the root`
2. reject a missing column, named → RED: `no variant named MissingColumn`
3. reject an undeclared column, named → RED: `no variant named UndeclaredColumn`
4. reject a type mismatch, both types named → RED: `no variant named TypeMismatch`
5. reject a looser nullability → RED: `no variant named NullabilityMismatch`
6. accept a stricter batch → written after the code, so **proven falsifiable instead**: inverting the
   nullability condition fails this test and the one above it, in both directions.

Then REFACTOR while green: four tests were rebuilding the same four columns, so the setup moved into
`document_columns()` and each test now changes only the one thing it is about. DRY for the setup, DAMP
for the deviation — the body of a test shows what makes it different.

## Verification (executable)

```bash
just format && just lint && just test && just qa

# every branch of the validator is guarded — break it, confirm a test notices
# (run from a clean tree; each mutation is reverted afterwards)
#   nullability condition inverted   -> 2 tests fail
#   type check removed               -> 1 test fails
#   undeclared-column check removed  -> 1 test fails
#   missing-column check removed     -> 1 test fails

cargo test --doc -p euledb-storage    # both doc examples execute, including the failing-insert one
```

## Out of scope / Guardrails

- **Nothing is written to disk.** Persistence is SUB-10, behind the storage trait.
- **No public re-export from the facade crate.** The published API surface is SUB-14.
- **No consolidated error type.** AC-71 in SUB-17 gathers every failure into one documented type, and
  `SchemaMismatch` becomes a variant of it then. Building that now would be guessing at variants that
  do not exist yet.
- Do not report all mismatches at once without deciding to — see the design note above.

## Definition of Done

- [x] AC-15 covered: a schema is an Arrow schema, a mismatching batch is rejected by column and reason
- [x] Every failing test observed failing first, or proven falsifiable where it was not
- [x] Every branch of the validator shown to be guarded by a mutation check
- [x] Doc examples compiled and executed by the suite
- [x] The duplicate-version policy applied by exception with a reason, not loosened
- [x] Commits follow Conventional Commits, grouped by concern
