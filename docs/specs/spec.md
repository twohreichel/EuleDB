---
spec: EuleDB
status: draft
created: 2026-08-21
scope: P0-P5 (full product)
source: docs/research/research-report-hybrid-database.md
---

# Spec — EuleDB

## Spine

**Problem.** Anyone who wants to search their own data by meaning, not just by keyword, currently has
to choose: a hosted vector service (data leaves the machine), a server-based store like Qdrant (~400 MB
resident, not embedded-first), or an embedded store like LanceDB that has no natural-language layer and
no multi-device sync. None of them can be queried by a non-expert — let alone a child — without writing
a query language, and none combines exact filters, semantic vector search and BM25 full-text in one
local, encrypted file.

**Goal.** One embedded Rust library, one encrypted file on disk, that fuses exact filters + vector
semantics + BM25 full-text, and that a non-expert can query in plain language through a validated,
sandboxed intermediate representation — fully offline, on commodity hardware, from a workstation down
to a low-power single-board computer. **No device is privileged**: the criteria below are stated in
platform classes, never in brands (§ Platform classes).

**Non-goals.**
- Not a server. No daemon, no network listener, no client/server protocol. Embedded library only.
- Not a general-purpose SQL engine. No joins, no transactions across tables, no query optimiser
  beyond the hybrid planner. Analytical workloads belong in DuckDB.
- Not a storage-format research project. Lance is the on-disk format (see § Decisions taken).
- Not a cloud sync service. Sync is peer-to-peer CRDT over a transport the caller supplies.
- Not a training or fine-tuning platform. Models are consumed, never trained.

## Glossary

The terms the criteria below are stated in. Where a term names a mechanism, the criterion that
governs it is cited.

| Term | Meaning in this project |
|---|---|
| **IR** | Query Intermediate Representation. The closed, typed enum the language model emits instead of SQL — the only thing the planner accepts (AC-40). |
| **Validator** | The deterministic gate between IR and plan. Fail-closed: an unknown field, operation or column, or a type mismatch, is rejected (AC-41). |
| **Hybrid query** | One query fusing three retrieval paths: exact filter, vector semantics and BM25 lexical (AC-27, AC-37). |
| **RRF** | Reciprocal Rank Fusion. `score(d) = sum_r 1/(k + rank_r(d))`, k = 60 by default. Fuses ranked lists without needing comparable score scales (AC-37, AC-38). |
| **HNSW** | Hierarchical Navigable Small World. Graph vector index for small and mid-size collections (AC-34). |
| **IVF-PQ** | Inverted File with Product Quantization. Memory-frugal vector index, the Class B choice (AC-35). |
| **ART** | Adaptive Radix Tree. Index for point and range lookups on key columns, after Leis, Kemper and Neumann 2013 (AC-24, AC-25). |
| **FSST** | Fast Static Symbol Table. String compression scheme, used by Lance internally for string columns (AC-19). |
| **KEK / DEK** | Key-Encryption Key, derived from the passphrase via Argon2id, wraps the Data-Encryption Key that actually encrypts the payload. Envelope encryption — rotating the DEK does not rewrite data (AC-20, AC-21). |
| **Capability token** | Signed grant carrying read, write or schema scope. The default is read-only (AC-6, AC-28). |
| **Auto-embedding column** | A declared text column embedded transparently on insert and update, kept consistent with its vector index without a caller step (AC-31). |
| **Prefix convention** | The E5 model family requires `query:` on queries and `passage:` on stored text. Omitting it degrades recall silently (AC-32). |
| **CRDT** | Conflict-free Replicated Data Type. The basis of serverless multi-device convergence (AC-51). |
| **Reference corpus** | The fixed, multilingual, redistributable dataset every KPI in AC-2 to AC-4 is measured on. Not yet chosen — see § Open questions. |
| **Class A / Class B** | Platform classes defined by capability, never by brand. See § Platform classes (AC-3, AC-4, AC-61). |
| **MSRV** | Minimum Supported Rust Version, pinned in `rust-toolchain.toml` and verified in CI (AC-11). |

## Shape

### User stories

1. As a **privacy-conscious developer**, I want to embed a hybrid search engine as a Rust crate, so that
   my users' data never leaves their device.
2. As a **non-technical user**, I want to ask my data a question in my own words and see what the system
   understood before it runs, so that I can trust the answer without learning a query language.
3. As a **child or beginner**, I want to assemble a query from visual blocks, so that I can search
   without depending on a language model getting my sentence right.
4. As an **operator on constrained hardware**, I want the engine to stay under a few hundred megabytes
   of RAM on a low-power machine, so that a sovereign local setup is affordable on whatever I own.
5. As a **multi-device user**, I want my database to converge across my devices without a server, so
   that local-first does not mean single-device.

### Acceptance criteria

Ids are flat across the whole product. **They freeze when `status:` moves from `draft` to `accepted`** —
from that point a new criterion appends at the end with its phase tag and nothing is ever renumbered,
because tests and PR bodies reference the id. While the spec is still `draft` and no test references an
id, renumbering for readability is allowed.

Every `AC-n` must end with at least one passing test; every task must name the `AC-n` it fulfils.

#### Cross-cutting — engineering

- **AC-1:** THE SYSTEM SHALL declare `#![forbid(unsafe_code)]` at every crate root. WHERE `unsafe` is
  unavoidable in an index hot path, it SHALL live in a module named in a TRACKED `UNSAFE.md` at the
  repository root with a stated invariant per block — a safety inventory belongs where a reader looks
  for it rather than one directory further in — and the crate-level `forbid` SHALL be downgraded to
  `deny` only there.
- **AC-2:** WHEN the reference corpus benchmark runs, THE SYSTEM SHALL achieve Recall@10 >= 0.90 against
  an exhaustive brute-force baseline computed over the same corpus.
- **AC-3:** WHEN a hybrid query runs without the language-model path, p95 latency SHALL be < 500 ms on
  ANY supported platform and < 100 ms on a Class A platform (§ Platform classes), measured over >= 1000
  queries on the reference corpus. The 500 ms figure is a universal ceiling, not a per-device target.
- **AC-4:** THE SYSTEM SHALL hold < 50 MB resident while idle and < 200 MB resident at query peak,
  measured on the reference corpus on EVERY supported platform. These ceilings are absolute and
  hardware-independent — that is what makes them portable.
- **AC-5:** THE SYSTEM SHALL publish every KPI in AC-2, AC-3, AC-4 as a reproducible in-repo benchmark
  that a third party can run with one documented command, and the recorded results SHALL name the
  hardware, corpus and commit they came from.
- **AC-6:** WHEN a query touches data, THE SYSTEM SHALL default to read-only. Any write, schema change
  or delete SHALL require an explicitly granted capability (see AC-28).

#### Cross-cutting — CI, supply chain, dependency management

- **AC-7:** WHEN a pull request is opened or updated, and WHEN a commit lands on the default branch, the
  pipeline SHALL run — as separately reported jobs — formatting check (`cargo fmt --check`), lint
  (`cargo clippy --all-targets --all-features -D warnings`), the test suite (`cargo nextest run`), and
  the documentation build (`cargo doc` with warnings denied, doctests executed). Each job SHALL be a
  required status check, so a merge is impossible while any of them fails.
- **AC-8:** THE SYSTEM SHALL fail the pipeline on a dependency advisory. `cargo deny check` (advisories,
  licences, bans, sources) and `cargo audit` SHALL run on every pull request AND on a weekly schedule,
  so that an advisory published after a merge still surfaces without a code change. The licence check
  SHALL fail on any dependency licence incompatible with `Apache-2.0 OR MIT`.
- **AC-9:** THE SYSTEM SHALL harden the workflows themselves: every third-party action pinned to a full
  40-character commit SHA with a version comment, `permissions: {}` at workflow level with least-
  privilege grants per job, no `pull_request_target` that checks out or executes fork code, no `${{ }}`
  interpolation of untrusted event data into a `run:` block (values pass through `env:`),
  `timeout-minutes` set on every job, and `persist-credentials: false` on checkout in every job that
  does not push. `actionlint` and `zizmor` SHALL run in the lint job so these are enforced
  mechanically rather than by review.
- **AC-10:** THE SYSTEM SHALL manage dependencies via Dependabot for both the `cargo` and the
  `github-actions` ecosystem, weekly, with patch and minor updates grouped and major updates ungrouped
  (one PR each). A cooldown SHALL delay version-update PRs after a release. There SHALL be no blanket
  `ignore` entry, because `ignore` suppresses security updates too. `github-actions` updates SHALL
  NEVER be auto-merged.
- **AC-11:** THE SYSTEM SHALL verify every commit on the platforms it claims to support: the suite SHALL
  run on linux-x86_64, linux-aarch64, macOS-arm64 and windows-x86_64, and against the MSRV pinned in
  `rust-toolchain.toml` as well as stable. A platform the matrix does not cover SHALL NOT be claimed as
  supported anywhere in the documentation.

- **AC-12:** THE SYSTEM SHALL derive the version number and the changelog from Conventional Commits
  via Release Please. WHEN commits land on the default branch, a release pull request SHALL be
  maintained that updates `CHANGELOG.md` and the crate version. No version number and no changelog
  entry SHALL be written by hand.
- **AC-13:** THE SYSTEM SHALL publish only from a tag created by a merged release pull request, and the
  publish job SHALL run after the gates of AC-7, AC-8 and AC-11 have passed on that commit. WHILE the
  version is below 1.0.0, a breaking change SHALL bump the minor version.
- **AC-14:** THE SYSTEM SHALL give a first-time contributor a documented path: a pull-request template
  carrying the author self-review gate, issue forms for bug reports and feature requests that collect
  version, platform and reproduction, and a `CONTRIBUTING.md` naming the four verification commands,
  the Conventional Commits requirement (AC-12 depends on it), the test-first rule and the
  issue-before-code rule for non-trivial changes. The public contribution surface SHALL NOT reference
  this specification or its `AC-n` ids: it has to stand on its own, and a contributor should not need
  to read the project's planning artefacts to know what is expected of a pull request.

- **AC-63:** THE SYSTEM SHALL expose a funding path for the project's sustainability: a
  `.github/FUNDING.yml` naming only platforms that are actually active, so the repository shows a
  sponsor button. A funding link that does not resolve SHALL be removed rather than left in place.
- **AC-64:** THE SYSTEM SHALL publish the community metrics the project is steered by — stars,
  contributor count, package downloads per month, and time to the first externally authored merged
  pull request — alongside the technical benchmarks of AC-5, recorded with the date they were taken.
  These metrics gate the phase-3 decision (see § Decisions taken) and are therefore evidence, not
  decoration.

- **AC-65:** THE SYSTEM SHALL carry complete crate metadata for publication — `description`,
  `license = "Apache-2.0 OR MIT"`, `repository`, `keywords`, `categories`, `readme` and `rust-version` —
  and `cargo publish --dry-run` SHALL pass before AC-13 attempts a release. A registry rejecting the
  publish at the tag is a failure discovered at the worst possible moment.
- **AC-66:** THE README SHALL state what EuleDB is for AND what it deliberately is not, because
  `CONTRIBUTING.md` delegates the scope question to it (AC-14). A contribution guide pointing at a
  scope statement that does not exist is a broken promise to the first contributor.

#### P0 — Storage foundation

- **AC-15:** THE SYSTEM SHALL define a table schema as an Apache Arrow schema and SHALL reject an insert
  whose record batch does not match the declared schema, naming the offending column and the mismatch.
- **AC-16:** WHEN rows are inserted, THE SYSTEM SHALL persist them in Lance format on disk and SHALL
  return them byte-identical on a subsequent scan after the handle is dropped and reopened.
- **AC-17:** THE SYSTEM SHALL expose the storage layer behind an internal trait boundary such that no
  module outside the storage crate references a Lance type, and the Lance dependency SHALL be pinned to
  an exact version.
- **AC-18:** THE SYSTEM SHALL compress stored blocks with zstd and SHALL make the compression level
  configurable per table at creation time.
- **AC-19:** THE SYSTEM SHALL encode string columns with FSST or dictionary encoding, and the
  documentation SHALL state whether that encoding is provided by the Lance layer or by an own encoder —
  no own implementation SHALL be written before the Lance behaviour has been measured.
- **AC-20:** WHEN a database is created with a passphrase, THE SYSTEM SHALL derive a key-encryption key
  via Argon2id and SHALL use it to wrap a separate, rotatable AES-256-GCM data-encryption key, persisted
  alongside the database. IF the passphrase is wrong, THEN unwrapping SHALL fail closed with a distinct
  error and the data-encryption key SHALL NOT be recoverable. (Encrypting the data itself with that key
  is AC-75 — see § Decisions taken.)
- **AC-21:** WHEN the data-encryption key is rotated, THE SYSTEM SHALL re-wrap the DEK without
  rewriting the encrypted payload, and previously written data SHALL remain readable.
- **AC-22:** IF an encrypted file is opened with the wrong passphrase, or IF any authentication tag
  fails, THEN THE SYSTEM SHALL fail closed with a distinct error and SHALL NOT return partial plaintext.
- **AC-23:** THE SYSTEM SHALL expose create-table, insert, scan, update, delete and drop through a
  documented Rust crate API. Python bindings are out of scope for P0 (see § Out of scope).
- **AC-67:** WHEN rows matching a predicate are updated, THE SYSTEM SHALL persist the new values and
  SHALL leave every non-matching row untouched, and a subsequent scan SHALL return the updated values
  after the handle is dropped and reopened.
- **AC-68:** WHEN rows matching a predicate are deleted, THE SYSTEM SHALL remove exactly those rows,
  SHALL report how many were removed, and SHALL log the affected count and the scoping predicate BEFORE
  executing — a delete broader than intended must be visible in the log, not inferred later from
  missing data.
- **AC-69:** IF the process is terminated at any point during a write, THEN reopening the database SHALL
  yield either the state before that write or the state after it — never a partially applied, torn or
  unreadable state. This SHALL be proven by tests that kill the writer at multiple points, not by
  argument.
- **AC-70:** THE SYSTEM SHALL permit multiple concurrent readers and at most one writer per database
  file, and SHALL reject a second writer with a distinct error rather than corrupting data or blocking
  indefinitely. The chosen model SHALL be documented on the public API.
- **AC-71:** THE SYSTEM SHALL return every failure as a value in one documented error type. The public
  API SHALL NOT panic on malformed input, a missing or unreadable file, a permission error or a failed
  decryption — a library that aborts its host process on bad data is not usable as an embedded
  component.

- **AC-75:** THE SYSTEM SHALL encrypt every byte of table data at rest with AES-256-GCM under the
  data-encryption key of AC-20, in independently addressable blocks, so that reading a range of a file
  does not require decrypting the whole file. A read whose authentication tag fails SHALL yield no
  plaintext for that block or any other.

- **AC-74:** THE SYSTEM SHALL expose every tunable through ONE documented configuration mechanism,
  each with a stated default and a stated effect. No behaviour SHALL be adjustable only by editing
  source or by an undocumented environment variable. The mechanism is established here, when the only
  tunables are storage-level, so that every later tunable (index type, fusion k, model choice, Class B
  tuning) slots into it instead of growing its own private channel.

#### P1 — Indices and exact queries

- **AC-24:** THE SYSTEM SHALL answer a point lookup on an indexed key column through a key index without
  a full scan, and SHALL prove this by an assertion on rows examined, not on wall-clock time.
- **AC-25:** THE SYSTEM SHALL answer a range predicate on an indexed key column through the same index,
  returning results in key order.
- **AC-26:** THE SYSTEM SHALL evaluate conjunctive and disjunctive filter predicates as Roaring bitmap
  set operations, and the result SHALL equal the result of the equivalent brute-force filter over the
  same data.
- **AC-27:** WHEN a query carries both an exact filter and a search clause, THE SYSTEM SHALL apply the
  exact filter as a pre-filter before candidate generation.
- **AC-28:** THE SYSTEM SHALL gate table and column access behind signed capability tokens carrying
  read, write or schema scope, and SHALL reject an operation whose token lacks the required scope
  without revealing whether the target exists.
- **AC-29:** WHEN any operation executes, THE SYSTEM SHALL by default append a record to a hash-chained,
  append-only audit log containing the query representation, the resolved plan and the number of rows
  affected. Reads are operations. Auditing SHALL be switchable off through the configuration mechanism of
  AC-74, because a read that must write cannot reach a database on read-only media — and the consequence
  of switching it off SHALL be stated where the tunable is.
- **AC-30:** IF any link of the audit-log hash chain does not verify, THEN THE SYSTEM SHALL report the
  index of the first broken link and SHALL refuse to append further entries until the chain is
  explicitly re-anchored.

#### P2 — Semantics and full text

- **AC-31:** WHEN a text column is declared as auto-embedding, THE SYSTEM SHALL embed it on insert and
  update and SHALL keep the vector index consistent with the column without an explicit caller step.
- **AC-32:** THE SYSTEM SHALL chunk text to the 512-token limit of `multilingual-e5-small`, SHALL apply
  the E5 prefix convention (`query:` for queries, `passage:` for stored text) and SHALL L2-normalise
  every vector before insert.
- **AC-33:** THE SYSTEM SHALL produce 384-dimensional embeddings via `multilingual-e5-small` in ONNX,
  and the same input SHALL yield a bit-identical vector across runs on the same platform.
- **AC-34:** THE SYSTEM SHALL support HNSW as the vector index for small and mid-size collections, with
  defaults M in 12..16 and M0 = 2*M, and cosine as the default distance.
- **AC-35:** THE SYSTEM SHALL support IVF-PQ as the vector index where memory is constrained, and the
  index type SHALL be selectable per table without changing the query API.
- **AC-36:** THE SYSTEM SHALL answer a BM25 full-text query via Tantivy, and the ranking SHALL be
  stable across identical runs.
- **AC-37:** WHEN a hybrid query runs, THE SYSTEM SHALL fuse the vector and BM25 candidate lists by
  Reciprocal Rank Fusion, `score(d) = sum_r 1/(k + rank_r(d))`, with k = 60 by default.
- **AC-38:** WHERE the corpus holds fewer than 100 documents, THE SYSTEM SHALL default k to a value in
  10..20, and the effective k SHALL be reported in the query explanation.
- **AC-39:** THE SYSTEM SHALL expose the fused result with per-source ranks, so that a caller can see
  whether a hit came from the vector side, the lexical side or both.

- **AC-72:** BEFORE the first public release, THE SYSTEM SHALL ship a getting-started document that a
  newcomer can follow end to end without prior knowledge of the project: install, create a table,
  insert data, and run each of the three query kinds — exact filter, semantic search, full text — plus
  one hybrid query. Every code example in it SHALL be compiled and executed by the test suite, so the
  guide cannot rot silently. A release without this is a library only its author can use.

#### P3 — Natural language to intermediate representation

- **AC-40:** THE SYSTEM SHALL define the query IR as a closed, typed, serde-serialisable enum covering
  at least `Filter`, `SemanticSearch`, `FullText`, `Sort` and `Limit`, and the language model SHALL emit
  only this IR — never SQL, never executable code.
- **AC-41:** WHEN the validator receives an IR document, THE SYSTEM SHALL reject any unknown field,
  unknown operation, unknown column or type mismatch, and SHALL fail closed with a message naming the
  rejected element.
- **AC-42:** THE SYSTEM SHALL refuse any destructive operation reached through the natural-language
  path by default, independent of what the model emitted.
- **AC-43:** WHEN a natural-language question is submitted, THE SYSTEM SHALL present a plain-language
  restatement of the understood query for confirmation before execution.
- **AC-44:** WHEN a result is returned, THE SYSTEM SHALL explain in plain language which query produced
  it, without jargon and without exposing internal identifiers.
- **AC-45:** THE SYSTEM SHALL NEVER include retrieved row content in a prompt position that the model
  can interpret as instruction; data and instruction SHALL occupy structurally separate channels.
- **AC-46:** WHILE no language model is available, THE SYSTEM SHALL fall back to a deterministic
  rule-based parser and SHALL state that it did so.
- **AC-47:** THE SYSTEM SHALL run the language model locally via llama.cpp (GGUF) on every supported
  platform, with no network call on the query path. WHERE a platform-specific accelerator is available,
  THE SYSTEM MAY use it as an optional faster path — never as a requirement, and never as the only
  path on that platform.
- **AC-48:** WHEN measured on the natural-language benchmark set, the rate of questions translated to a
  correct IR SHALL be >= 85 % with a 4-8B parameter model and >= 70 % with a 1-3B parameter model at
  Q4_K_M. The threshold follows the MODEL SIZE, not the machine — which machine can run which size is
  a deployment question, not a correctness one.
- **AC-49:** IF the correct-IR rate for a given platform-and-model combination measures below 60 %,
  THEN THE SYSTEM SHALL ship with the language-model path disabled for that combination and the
  rule-based parser (AC-46) as the default there. The kill switch is per combination, so a weak
  platform degrades gracefully instead of degrading the product.
- **AC-50:** THE SYSTEM SHALL keep the IR validation failure rate below 1 % of submitted IR documents on
  the benchmark set.

#### P4 — CRDT sync

- **AC-51:** THE SYSTEM SHALL represent syncable state as Loro CRDT documents such that two replicas
  that have exchanged all deltas converge to an identical state regardless of delta order.
- **AC-52:** WHEN two replicas modify the same row concurrently, THE SYSTEM SHALL converge without
  operator intervention and SHALL record the resolution in the audit log.
- **AC-53:** THE SYSTEM SHALL emit sync deltas encrypted under the same AES-256-GCM scheme as data at
  rest, and a delta SHALL be rejected if its authentication tag fails.
- **AC-54:** THE SYSTEM SHALL be transport-agnostic: sync SHALL accept any byte-oriented transport the
  caller supplies and SHALL NOT open a socket itself.
- **AC-55:** WHEN a replica has been offline, THE SYSTEM SHALL converge on reconnect by exchanging
  deltas only, without a full document transfer.

#### P5 — User experience and maturity

- **AC-56:** THE SYSTEM SHALL offer a Blockly-style visual block editor whose blocks map one-to-one onto
  the IR enum variants of AC-40, and the editor SHALL be structurally incapable of producing invalid IR.
- **AC-57:** THE SYSTEM SHALL offer all three query modes — natural language, visual blocks, and
  confirmation preview — against the same IR and the same planner.
- **AC-58:** WHEN an error reaches the user, THE SYSTEM SHALL phrase it without jargon and SHALL name
  the next action the user can take.
- **AC-59:** THE SYSTEM SHALL expose the P0-P2 surface to Python via PyO3, with zero-copy Arrow exchange
  to PyArrow, Pandas and Polars.
- **AC-60:** THE SYSTEM SHALL publish abi3 wheels built by maturin for every platform in the AC-11
  matrix — manylinux x86_64, manylinux aarch64, macOS-arm64 and Windows x86_64 — and a documented smoke
  test SHALL pass against each published wheel.
- **AC-61:** THE SYSTEM SHALL meet AC-3 and AC-4 on a Class B platform (§ Platform classes) with the
  tuned configuration, and the tuning SHALL be documented and selectable rather than hardcoded or
  auto-detected from a device name.
- **AC-62:** THE SYSTEM SHALL document every public API item, and the documented examples SHALL be
  compiled and executed by the test suite. This is REFERENCE documentation — it answers "what does this
  function do", never "how do I get started".
- **AC-73:** THE SYSTEM SHALL ship user documentation covering four distinct needs, because a single
  page cannot serve them: (a) **installation** for every platform in the AC-11 matrix, including the
  model files the semantic and natural-language paths need; (b) a **configuration reference** listing
  every tunable of AC-74 with its default and its effect; (c) **task-oriented guides** — one per
  capability a user actually has, written as "how do I …"; (d) an **explanation** of why hybrid search
  fuses three paths and when each one wins, because a user who does not understand the model cannot
  judge a bad result. Four rules govern it: no unfilled or placeholder section, no boilerplate that is
  identical in every Rust project, no speculative operations content, and no section kept for symmetry
  — delete a section rather than pad it.

### Out of scope

- **Python bindings before P5** — deferred by explicit decision; P0-P4 deliver a Rust crate API only
  (AC-23). Risk accepted and recorded below.
- **Cross-encoder reranking** — named in the research as a later feature; no criterion until the RRF
  baseline in AC-37 is measured.
- **CJK tokenisation** — Tantivy needs a third-party tokeniser (`tantivy-jieba`, `cang-jie`, `lindera`);
  deferred until a concrete corpus demands it.
- **Embedding model upgrade to `multilingual-e5-base` / `-large`** — the upgrade path stays open by
  keeping the model selectable, but only `small` is a supported target.
- **Schema evolution.** Adding, removing or retyping a column on an existing table. Real databases need
  it, and it is deliberately deferred: the migration semantics depend on how Lance versions data, which
  is only understood after AC-13. Until then, changing a schema means creating a new table and copying.
  This is a known limitation, not an oversight — it must be documented as such (AC-62).
- **Backup and restore tooling.** The database is a file, so copying it while no writer holds it is a
  backup. A consistent online backup, incremental backup or point-in-time restore is out of scope until
  someone needs it.
- **Trademark clearance for the name** — the research flags registry checks as partly unverified. A
  separate task, not a code criterion.

## Technology stack

The inventory the acceptance criteria deliberately do not contain: a criterion states observable
behaviour, a crate choice is an implementation decision that belongs here and may change without
touching a criterion. **Status legend** — `set`: decided, change needs a reason. `evaluate`: two or more
candidates named, decision deferred to the phase that needs it.

| Layer | Concern | Choice | Status | Note |
|---|---|---|---|---|
| L0 | In-memory format | `arrow-rs` | set | Arrow is the interchange contract for AC-15 and AC-59 |
| L0 | On-disk format | `lance` | set | pinned exactly, behind the trait of AC-17 |
| L0 | Block compression | `zstd` | set | AC-18 |
| L0 | String encoding | FSST / dictionary | evaluate | Lance encodes strings internally — measure before writing an own encoder (AC-19) |
| L0 | Encryption | `aes-gcm` (RustCrypto) | set | AES-256-GCM, one NCC Group audit, no significant findings, AES-NI/CLMUL accelerated |
| L0 | Key derivation | `argon2` | set | Argon2id, AC-20 |
| L1 | Key index | Lance-native scalar index | set | decided at P1, see below. The Adaptive Radix Tree it replaces is recorded under § Decisions taken |
| L1 | Predicate sets | `roaring` | set | the official roaring-rs port, AC-26 |
| L1 | Vector index (HNSW) | Lance-native | set | decided at the P2 cut: it ships HNSW with cosine as well as IVF-PQ, so an extra crate is unnecessary. See § Decisions taken |
| L1 | Vector index (IVF-PQ) | Lance-native | set | AC-35 |
| L1 | Full text | `tantivy` | set | BM25 as in Lucene, stemming for 17 Latin languages, <10 ms startup |
| L1 | Embedding model | `intfloat/multilingual-e5-small` | set | 384 dim, 100 languages, 512 tokens, 12 layers |
| L1 | Inference runtime | `candle-onnx` | set | decided at SUB-28 on supply-chain grounds and confirmed by running the graph. See § Decisions taken |
| L2 | Fusion | own RRF, k = 60 | set | no crate needed, AC-37 |
| L3 | IR serialisation | `serde` | set | AC-40 |
| L3 | Model runtime | llama.cpp (GGUF) everywhere; platform accelerators optional | set | AC-47 |
| L3 | Model family | Qwen — 4-8B where memory allows, 1-3B Q4_K_M otherwise | set | sized by available RAM, not by device |
| L4 | CRDT | `loro` | set | AC-51 to AC-55 |
| L5 | Python bindings | `pyo3`, `maturin`, `pyo3-arrow` | set | deferred to P5 by decision; `pyo3-arrow` provides the zero-copy of AC-59 |
| L5 | Block editor | Blockly | set | AC-56 |
| CI | Supply chain | `cargo-deny`, `cargo-audit`, Dependabot | set | AC-8, AC-10 |
| CI | Workflow lint | `actionlint`, `zizmor` | set | AC-9 |
| CI | Test runner | `cargo-nextest` | set | AC-7 |
| CI | Versioning + changelog | `release-please` (release-type `rust`, `cargo-workspace` plugin) | set | AC-12, AC-13 |
| CI | Contribution surface | `.github` PR template + issue forms + `CONTRIBUTING.md` | set | AC-14 |

## Constraints

### Platform classes

Referenced by AC-3, AC-4, AC-11 and AC-61. **Defined by capability, never by brand** — a class must
stay meaningful when today's hardware is superseded.

| Class | Definition | Expectation |
|---|---|---|
| **A — capable** | >= 4 performance cores, wide SIMD (AVX2 or NEON), >= 8 GB RAM | the tighter targets: p95 < 100 ms |
| **B — constrained** | any other supported platform: <= 4 cores, >= 2 GB RAM, CPU-only inference | the universal ceiling: p95 < 500 ms, same RAM limits |

Two consequences that are easy to get wrong:

- **No GPU offload may be assumed anywhere.** Many low-power targets have no llama.cpp backend for
  their GPU at all, so CPU-only is the baseline everywhere and any accelerator is a bonus path (AC-47).
- **Class membership is measured, not detected.** The benchmark records the actual core count, SIMD
  width and RAM (AC-5). Deriving a class from a device string would reintroduce exactly the
  device-specific behaviour this section removes.

### Decisions taken

**The inference runtime is `candle-onnx`** (decided 2026-08-22, at SUB-28). The choice was framed as op
coverage against build complexity. What actually decided it is supply chain: **`ort`'s default features
download a prebuilt C++ ONNX Runtime at build time over TLS**, and that artefact sits outside every gate
this project has — `cargo-deny` inspects Rust crates, and CI pins third-party actions to commit SHAs
precisely so that no unverified mutable artefact enters a build. Using `ort` without that feature instead
requires a native ONNX Runtime installed on every developer machine and all four CI platforms, which is a
much heavier operational burden than the one build tool already required. `ort` 2.0 is also still a release
candidate, and the embedding path is not the place for one.

The op-coverage worry was settled by measurement rather than argument: `candle-onnx` loads this exact graph
and returns `last_hidden_state` with shape `[1, n, 384]`. It needs `protoc`, which the on-disk format
already requires, so the build-time cost is one this project pays anyway.

**Two costs, both recorded.** The dependency tree grows by about 195 crates against `ort`'s 51 — seven new
duplicate-version entries were added to `deny.toml` with reasons, and one duplicate (`tokenizers`) was
*removed* by aligning the declaration to what `candle-core` already uses rather than skipped. And the
**minimum supported Rust version rises from 1.91.0 to 1.94.0**: `candle-core` declares no `rust-version`
while using an unstable feature on aarch64, so its requirement was measured — 1.93.0 fails, 1.94.0 builds.
The MSRV was always derived from what dependencies need rather than chosen, so this is the same rule
applied to a dependency that does not declare its own.

**Mean pooling, not the leading token** (noted at SUB-28). E5 is trained with mean pooling, and pooling the
first token instead produces vectors that are stable, cheap and not the model's. No shape, normalisation,
prefix or determinism assertion can tell the difference — only retrieval can, which is why the pipeline's
test set includes a semantic one: a query must sit measurably closer to the passage answering it than to an
unrelated passage.


**The vector indices are the format's too** (decided 2026-08-22, at the P2 cut). The pinned format ships
HNSW with an `m` parameter and cosine distance, plus `IvfPq`, `IvfHnswPq` and `IvfRq`. AC-34 and AC-35
describe behaviour — HNSW for small and mid-size collections with documented defaults, IVF-PQ where memory
is constrained, selectable per table without changing the query API — and the format supplies both. Taking
them is the same decision as the scalar index in P1, for the same reason and at the same recorded cost: a
deeper coupling to a dependency the trait boundary keeps replaceable in name. The alternative, a separate
HNSW crate, is not chosen speculatively; if a measurement at SUB-30 shows the format's recall or its
parameter range cannot meet AC-34, that is the moment to revisit.

**Full text stays Tantivy, pending one check** (noted 2026-08-22). The format also ships an inverted index,
so the ART argument applies in principle. It is *not* being applied here without measurement, because the
stack table's reason for Tantivy is stemming across 17 Latin languages and this database is multilingual —
a full-text engine that cannot stem German or Polish is not a substitute whatever else it saves. SUB-32
checks what the format's inverted index does for stemming and records the comparison either way. This is
the one place in the project where a second engine in the tree may be the right answer.


**No scalar index in this format returns rows in key order** (measured 2026-08-22, at SUB-21). AC-25 is
phrased so that a reader assumes the index supplies the order. It does not: a range over an indexed
column was run against both the ordered and the bitmap index kind, and both returned storage order. The
criterion is met by narrowing with the index and then ordering the rows it selected — a sort over the
matches, not over the table, which is why narrowing first is what keeps it cheap. Measured, not argued:
a full scan followed by a sort returns the same rows in the same order, so the rows-examined count is
what tells the two apart.

**Consequence, worth stating because it will be asked:** the choice of index kind is therefore not
distinguishable by any P1 criterion. Neither kind provides ordering and both serve a range without a
full scan. The ordered kind is chosen on cardinality — a bitmap over a thousand distinct integers is a
thousand bitmaps — and if a low-cardinality column ever wants the other kind, that is a tunable for the
mechanism of AC-74, not a criterion.


**Reads are audited, and auditing can be switched off** (decided 2026-08-22, at the P1 cut). AC-29 asks
for a record per operation and AC-70 fixes many readers against one writer, so a reader that appends needs
a lock — and a "read-only" handle that writes cannot open a database on read-only media at all. Resolved
in three parts: the log takes a short-lived exclusive lock **on its own file**, so readers serialise only
for the duration of one append and AC-70 is untouched; the default records every operation, reads
included; and auditing is a tunable on the one configuration mechanism, so the read-only-media case stays
usable. Rejected: auditing only data-changing operations, which would have read AC-29 narrower than it is
written without saying so.

**Capability tokens are symmetric** (decided 2026-08-22, at the P1 cut). HMAC-SHA256 under a key derived
from the key-encryption key of AC-20, with a distinct derivation context so a token key can never be a
data key. This document describes no second party — no multi-user model, no delegation, no tenancy — so an
issuer that can also verify costs nothing, and asymmetric signatures would add a dependency and a second
key to manage for a boundary that does not exist. Revisit if a token is ever issued across one.

**The key index needs no separate persistence decision** (noted 2026-08-22). Whether to hold a key index
in memory or on disk was a live question only while the index was going to be ours. The format's index is
persisted, which is the better of the two options at no cost, so the question is closed by the decision
above rather than answered on its own.


**The key index is the format's, not an Adaptive Radix Tree of ours** (decided 2026-08-22, at the P1
cut). AC-24 and AC-25 named an ART after Leis, Kemper and Neumann. Reading the pinned format's own crates
showed it already ships a **persisted** scalar index answering both an exact lookup and a range as a pair
of bounds, returning stable row ids, and reached through the object store this project already encrypts —
so persistence and encryption at rest cost nothing.

Building an ART instead would mean a large unsafe-heavy dependency and an index rebuilt in memory on every
open, to duplicate what the format persists. Both criteria are now written as the behaviour they were
always about — a point lookup and an ordered range without a full scan, measured on rows examined — which
is what this document's own rule demands: a criterion that names an implementation is not falsifiable as
behaviour and locks that implementation in.

**The cost, stated:** this deepens the coupling to the format. The trait boundary still holds — no type of
the format's leaves the storage crate — but replacing the format would now mean replacing its index too.
The alternative was rejected because the boundary was never a promise that nothing behind it would be
used, only that nothing behind it would leak.


- **Lance as on-disk format, not an own format.** The competitive advantage is the NL/IR layer, the
  sandbox, the fusion planner, CRDT sync and the UX — not the storage format. Saves an estimated 3-5
  person-months. Mitigated by AC-17 (trait boundary + pinned version) because Lance is under active
  development.
- **Rust crate API first, Python later.** Deliberate deviation from the research document, which put a
  PyO3 skeleton in P0. Keeps the build matrix out of the way while the API churns.
- **Licensed `Apache-2.0 OR MIT`, copyright holder "Andreas Reichel".** The Rust ecosystem default dual licence: maximum adoption and
  explicit patent protection via Apache-2.0. Permits proprietary embedding, which is accepted.
- **`multilingual-e5-small` as the default embedding model everywhere.** The only model that is
  practical across the whole supported range including Class B; 384 dimensions, 100 languages,
  512-token limit.
- **Model size is chosen by available memory, not by device.** Qwen 4-8B where RAM and compute allow,
  Qwen 1-3B at Q4_K_M otherwise, plus a rule-based fallback everywhere. Community benchmarks put a 7B
  model on a low-power SBC under 2 tok/s, which is why the small tier exists at all.
- **The name is claimed by publishing a `0.0.0` placeholder in SUB-2, not later.** Verified free on
  2026-08-21: `euledb` returned 404 on crates.io, PyPI and npm. Neither registry has a reservation
  mechanism, so publishing is the only claim. Accepted cost: a public artifact exists before there is
  anything to use, and crates.io never deletes a version — a yank stays visible. Accepted because the
  alternative leaves a generic name unclaimed for roughly a year.
- **No privileged reference device.** An earlier draft anchored the KPIs to two specific machines. That
  makes every criterion stale the moment the hardware changes and quietly excludes every other user.
  Criteria now use platform classes, and portability is proven by the CI matrix (AC-11) rather than
  asserted about one device.
- **RRF with k = 60 as the fusion default.** Cormack, Clarke and Büttcher, SIGIR 2009; the de-facto
  default in Elasticsearch, OpenSearch, Weaviate, Qdrant and Azure AI Search. Scale-independent, so it
  tolerates incompatible BM25 and cosine ranges.
- **First public release after P2.** Collect community feedback before the expensive NL layer. The
  research sets a continue threshold of >= 50 GitHub stars or >= 3 external interested parties with a
  concrete use case.
- **AC-20 was split, and AC-75 appended, because it bundled two independently deliverable things.**
  As written it required both the key hierarchy (Argon2id key-encryption key wrapping a rotatable
  data-encryption key) and the encrypted data path (every byte at rest under that key). Those are
  different sizes of problem: the key hierarchy is self-contained and testable in isolation, while the
  data path needs a block-framed AEAD behind the storage format's object-store hook and is
  security-critical code. Delivering them in one change would mean a single large diff of cryptography
  reviewed in one sitting, which is the arrangement most likely to let a defect through. AC-20 now
  covers the key hierarchy including failing closed on a wrong passphrase, AC-75 covers the data path,
  and AC-22 stays with the data path where authentication tags are actually verified. See
  `docs/adr/ADR-002-where-encryption-sits.md`.
- **The planning tree is tracked under `docs/`, not kept local.** Specification, decision records,
  backlog and the research report moved out of the ignored `.vscode/` tree. The specification is the
  durable artifact of spec-driven development and the code is the regenerable part, so an untracked
  source of truth is a contradiction — and it survives no machine change. Two criteria argued from the
  old arrangement and were corrected with the move: AC-1 (`UNSAFE.md` stays at the repository root
  because that is where a reader looks for a safety inventory, not because `docs/` is unreadable) and
  AC-14 (the contribution surface stays self-contained because it must stand on its own, not because
  the specification is invisible). The accepted cost is that planning is now public, including the
  risks carried and the open questions.
- **Crate choices live in § Technology stack, not in criteria.** A criterion that names a crate is not
  falsifiable as behaviour and locks in an implementation. Swapping a crate must not require editing an
  `AC-n`.

### Risks carried

- **Later-phase criteria will be revised.** AC-40 to AC-62 are written against today's understanding.
  Criteria for the NL layer in particular are the least grounded and are expected to change once P2 is
  measured. They are recorded as intent, not as settled contract.
- **Deferring PyO3 to P5 delays discovery of binding problems** — abi3 constraints and Arrow zero-copy
  behaviour surface late. Accepted consequence of the Rust-first decision.
- **The house conventions this project inherits were written for other languages.** What carries over
  is language-neutral — test-first, small functions, validation at the boundary, no silent degradation.
  What does not carry over is every tool mandate in them, so the gate is stated here instead of assumed:
  `cargo fmt`, `cargo clippy -D warnings`, `cargo nextest run` and `cargo deny`, behind the four `just`
  targets named in `CONTRIBUTING.md`.
- **Class B numbers are measured on whatever CI provides, and that is a weaker guarantee than a
  device-specific promise.** A hosted aarch64 runner has a different core count, memory bandwidth and
  thermal behaviour from any particular single-board computer. The mitigation is honesty rather than
  precision: AC-5 records the actual hardware with every figure, so a reader can judge transferability
  instead of trusting a label. A self-hosted runner would be closer to real constrained hardware but is
  ruled out by AC-9 — on a public repository any fork pull request can poison a persistent machine.
- **Windows is now a claimed platform, and its cost is deferred.** Pure Rust makes it nearly free in
  P0 and P1. The real work lands in P2 (an ONNX runtime that builds there) and P3 (llama.cpp). If that
  proves disproportionate, the honest fix is to remove Windows from AC-11 and stop claiming it — not to
  claim it untested.
- **P0 grew after the fact, and that is the honest cost of a late review.** Update, delete, crash
  safety, the concurrency model and the public error type were missing from the original concept and
  therefore from the first draft of this spec. They are not optional extras — without them there is no
  database — so P0 is now larger than the 2-3 person-months the research estimated for it. Treat that
  estimate as superseded rather than pretending the scope fits it.
- **One maintainer is the largest sustainability risk, and it is structural.** 17-21 person-months of
  scope on a single pair of hands means the project dies of maintenance load long before it dies of a
  technical problem. The countermeasures are design constraints, not good intentions: prefer
  conservative, audited, widely-used crates (`aes-gcm`, `roaring`, `tantivy`, `lance`) over the newest
  option; keep module boundaries sharp enough that any one layer can be handed over or rewritten
  alone; and keep the build matrix small — abi3 wheels (AC-60) exist partly for this reason. AC-63
  (funding) and AC-64 (community metrics) are mitigations of this risk, not vanity features. **A new
  dependency is a maintenance obligation, so it carries the same written justification a new
  abstraction does.**
- **SHA-pinning actions costs automatic security backports.** AC-9 buys immutability against a retag
  attack (`tj-actions/changed-files`, CVE-2025-30066, ~23 000 repositories) and pays for it by needing
  Dependabot to deliver fixes. Accepted. Consequence: an action bump is reviewed as a source diff
  between the two SHAs, never merged on the version comment alone.

### Effort and schedule

**Deliberately not in this specification.** A criterion states observable behaviour; an estimate states
a schedule, and mixing them makes the spec rot on contact with reality. The research estimate — 2-3
person-months for P0, 2-3 for P1, 3-4 for P2, 3-4 for P3, 2-3 for P4, 3-4 for P5, so **17-21 in total
for one experienced developer** — lives on the phase tickets in `docs/backlog/`, where planning
belongs. Those numbers assume the chosen crates hold up and the UX scope does not grow.

### Open questions

- ~~**`ort` vs `candle` as the ONNX runtime.**~~ **Closed 2026-08-22** — `candle-onnx`, on supply-chain
  grounds and confirmed by running the graph. See § Decisions taken.
- ~~**Which reference corpus grounds AC-2, AC-3, AC-4?**~~ **Closed 2026-08-22** — a fixed window of the
  dated `20231101` Wikipedia snapshot in four languages, documented with its licence and digest in
  `corpus/README.md`.
- **Which natural-language benchmark set grounds AC-48 and AC-50?** Blocking for P3 only.
- ~~**Does Lance's own vector index remove the need for a separate HNSW crate?**~~ **Closed 2026-08-22** —
  it does: the format ships HNSW with cosine as well as IVF-PQ. Revisit only if a measurement at SUB-30
  shows it cannot meet AC-34.
- **The MSRV has no value yet.** AC-11 verifies against "the MSRV pinned in `rust-toolchain.toml`", but
  no version is chosen. It cannot be picked freely: it is the maximum of what `lance`, `tantivy`,
  `arrow-rs` and `aes-gcm` require. Determine it in SUB-2 by reading their manifests, not by guessing.
- **ADR for the Lance decision.** It satisfies all three Pocock conditions (hard to reverse, surprising
  without context, real trade-off), so `docs/adr/ADR-001-lance-as-storage-format.md` is recommended
  before the storage subticket lands.
