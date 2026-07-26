# Durable implementation checklist

Status: **Runnable CLI MVP complete; Batch 3 statement coverage not started**

Update this file during every batch. A checked feature requires focused tests
and a self-review; syntax support also requires a fixture.

## Global gates

- [x] Preserve the original handoff, style guide, and ZIP with checksums.
- [x] Install one canonical repository-scoped agent skill.
- [x] Record architecture and upstream baseline.
- [x] Add repository development instructions.
- [x] Initialize a Git repository.
- [x] Preserve the repository owner's existing MIT license.
- [x] Keep formatter source code out of Batch 0.
- [x] Commit Batch 0 before formatter implementation.
- [x] Keep formatter API reusable by CLI, stdin, Go extraction, and IDEs.
- [x] Maintain parse-before/after and idempotence invariants in the formatter
  facade; no-partial-write remains a Batch 4 filesystem gate.
- [x] Do not claim syntax support without a fixture.

## Batch 0 — Durable specification

- [x] Read `semantic-block-sql-work-handoff.md` completely.
- [x] Read `semantic-block-sql-style-guide-ru.md` completely.
- [x] Read every file in `postgresql-sql-format.zip`.
- [x] Save normalized Markdown copies in `docs/`.
- [x] Save the original ZIP in `docs/source/`.
- [x] Verify byte-identical SHA-256 checksums.
- [x] Extract the canonical skill to `.agent-skills/postgresql-sql-format/`.
- [x] Add Codex and Claude discovery symlinks.
- [x] Create `docs/formatter-design.md`.
- [x] Create `docs/upstream-baseline.md`.
- [x] Create `AGENTS.md`.
- [x] Create this durable checklist.
- [x] Verify current upstream manifests, commits, licenses, and MSRVs.
- [x] Record initial `libpgfmt` extension points and blockers.
- [x] Perform Batch 0 self-review.
- [x] Commit and tag the batch as `batch-0`.

Batch 0 evidence:

- No `Cargo.toml` or `src/` exists.
- Uploaded Markdown and ZIP SHA-256 values are in `docs/source/README.md`.
- Upstream commit identifiers are in `docs/upstream-baseline.md`.
- The commit carrying this checklist is the Batch 0 commit; tag `batch-0`
  identifies it without a self-referential commit hash in this file.

## Batch 1 — Upstream spike

### Environment and source

- [x] Obtain Rust 1.88+ and record `rustc --version --verbose`.
- [x] Obtain Cargo and record `cargo --version`.
- [x] Fetch exact `libpgfmt v1.3.0` source.
- [x] Verify source tag/commit and BSD-3-Clause notice.
- [x] Run complete upstream tests unchanged.
- [x] Record baseline test results and timing.

### Characterization

- [x] Add characterization cases for inline and standalone comments.
- [x] Add characterization cases for invalid SQL and tolerated `ERROR` nodes.
- [x] Add characterization cases for multiple statements and no trailing
  semicolon.
- [x] Add characterization cases for dollar quoting and PL/pgSQL.
- [x] Add characterization cases for compact and complex joins.
- [x] Add characterization cases for authored result-list line groups.
- [x] Add a characterization case for `MERGE`.
- [x] Verify whether original source spans/group hints remain usable.

### Minimal Semantic Block proof

- [x] Add `Style::SemanticBlock` in the spike.
- [x] Uppercase keywords and special values.
- [x] Lowercase ordinary function and type names safely.
- [x] Use four-space indentation.
- [x] Keep a simple `SELECT` compact.
- [x] Expand mixed `AND`/`OR` visibly.
- [x] Render expanded `JOIN ... ON` without an indentation storm.
- [x] Preserve comments in all spike fixtures.
- [x] Parse formatted output.
- [x] Require idempotence.

### Decision

- [x] Compare upstream patch, pinned fork, and vendoring.
- [x] Document exact required internal/public API changes.
- [x] Decide and record backend strategy.
- [x] Record dependency pin and update policy.
- [x] Run Batch 1 focused and full tests.
- [x] Perform Batch 1 self-review.
- [x] Update this checklist and design decisions.
- [x] Remove spike dead code.
- [x] Commit Batch 1.

Batch 1 evidence:

- `docs/batch-1-backend-spike.md` records exact upstream versions, commands,
  results, blockers, patch/fork analysis, and the backend decision.
- `tests/fixtures/batch1/` contains every layout shape claimed by this batch.
- `tests/batch1_semantic_block.rs` requires PostgreSQL parse-before/after,
  protected-token preservation, canonical AST equality, and idempotence.
- The commit carrying this checklist is the coherent Batch 1 commit.

## Batch 2 — Core layout

### Layout engine

- [x] Define `FormatOptions` and stable formatter facade.
- [x] Implement compact/expanded decisions.
- [x] Implement configurable four-space indentation.
- [x] Implement soft line width.
- [x] Implement hard line width and safe-break enforcement.
- [x] Allow indivisible over-hard tokens with diagnostics/metadata as designed.
- [x] Implement trailing commas.
- [x] Implement binary/cast/function spacing.
- [x] Implement `<>` to `!=` lexical normalization without touching literals or
  comments.

### Authored groups and comments

- [x] Capture original line boundaries as soft list-group hints.
- [x] Preserve blank lines as hard group boundaries.
- [x] Preserve comments as hard group boundaries.
- [x] Prevent merges across blank lines/comments.
- [x] Preserve cohesive over-soft groups.
- [x] Split over-hard groups only at safe argument boundaries.
- [x] Deterministically pack simple one-line input to soft width.
- [x] Place complex arguments on independent lines.
- [x] Preserve comment attachment to the same syntax node.

### Core constructs

- [x] Compact and expanded `SELECT`.
- [x] Boolean expressions with mixed `AND`/`OR`.
- [x] Compact and expanded joins.
- [x] Compact and expanded `CASE`.
- [x] CTEs.
- [x] Recursive CTE anchor/recursive branches.
- [x] Golden fixtures for every supported shape.
- [x] Idempotence property tests.
- [x] Parse-before/after tests.
- [x] Hard-limit property tests.
- [x] Batch 2 full tests and self-review.
- [x] Dead-code removal, docs, checklist, and commit.

Batch 2 evidence:

- `docs/batch-2-core-mvp.md` records the implemented scope, algorithm,
  warnings, limitations, and verification commands.
- `tests/fixtures/batch2/` covers authored groups, comments, width packing,
  compact/expanded `CASE`, function arguments, multiple CTEs, and a recursive
  CTE.
- `tests/batch2_core_layout.rs` requires PostgreSQL structural equivalence,
  protected-token preservation, idempotence, configurable indentation,
  compact/expanded JOIN width behavior, hard-width enforcement, and
  indivisible-token warnings.
- Batch 2 adds no dependency; layout continues to consume the scanner and parse
  tree supplied by the exactly pinned `pg_query`.
- The commit carrying this checklist is the coherent Batch 2 commit.

## Batch 3 — PostgreSQL statement coverage

- [ ] `INSERT`.
- [ ] Simple and complex `VALUES`.
- [ ] `ON CONFLICT` target predicate.
- [ ] `ON CONFLICT DO UPDATE` action predicate.
- [ ] `UPDATE ... FROM`.
- [ ] `DELETE ... USING`.
- [ ] `RETURNING`.
- [ ] Compact and expanded `MERGE`.
- [ ] `UNION`, `UNION ALL`, `INTERSECT`, and `EXCEPT`.
- [ ] Lateral joins.
- [ ] Grouping and `HAVING`.
- [ ] Window expressions and named windows.
- [ ] `FILTER`.
- [ ] PostgreSQL casts, arrays, and JSON expressions.
- [ ] `CREATE TABLE`.
- [ ] Simple one-line `CREATE INDEX`.
- [ ] Simple one-line partial index.
- [ ] Complex multiline `CREATE INDEX`.
- [ ] `ALTER TABLE`.
- [ ] PL/pgSQL blocks.
- [ ] Dollar quoting.
- [ ] Comments for each statement family.
- [ ] Golden, parse, idempotence, and safety fixtures for every checked item.
- [ ] Batch 3 full tests and self-review.
- [ ] Dead-code removal, docs, checklist, and commit.

## Batch 4 — Project CLI

Batch 4 and the raw-string subset of Batch 5 were implemented together as one
runnable MVP slice after Batch 2. Batch 3 remains deliberately deferred; this
does not expand the formatter-core syntax claims.

### Contract

- [x] `semblock fmt`.
- [x] `semblock check`.
- [x] `semblock diff`.
- [x] Stable documented exit codes.
- [x] `--config`.
- [x] `--stdin`.
- [x] `--filename`.
- [x] `--language auto|sql|go`.
- [x] `--jobs`.
- [x] `--verbose`.
- [x] `--quiet`.

### Configuration and discovery

- [x] Parse and validate `semblock.toml`.
- [x] Document config search and precedence.
- [x] Reject invalid widths and unsupported dialects.
- [x] Recursively discover `.sql`.
- [x] Respect `.gitignore` by default.
- [x] Support `.semblockignore`.
- [x] Document and test ignore precedence.
- [x] Deliberately configure hidden-file behavior.
- [x] Handle explicit ignored paths consistently.

### Safety and diagnostics

- [x] SQL file-level ignore.
- [x] SQL block off/on state machine.
- [x] Diagnostics for unmatched/nested/misplaced directives.
- [x] Original parse.
- [x] Formatted parse.
- [x] Idempotence gate.
- [x] Atomic replacement preserving permissions.
- [x] No partial file writes.
- [x] Unified diff output.
- [x] Preserve newline convention where practical.
- [x] Integration tests for files, directories, ignores, stdin, commands, exit
  codes, parse failure, and atomicity.
- [x] Batch 4 full tests and self-review.
- [x] Dead-code removal, docs, checklist, and commit.

Batch 4 evidence:

- `tests/cli_mvp.rs` covers the three commands, stdin, configuration,
  recursive discovery, both ignore sources, nested custom ignores, explicit
  ignored paths, stable exit classes, unified diff, CRLF, permissions, and
  pre-write planning.
- Discovery uses pinned `ignore 0.4.31`; `.semblockignore` is registered as a
  highest-precedence custom ignore filename. `.gitignore` is deliberately
  applied even when an explicit traversal root is not itself a Git repository.
- The CLI plans and validates every source before the first `fmt` write, then
  replaces each changed file through a same-directory temporary file.

## Batch 5 — Go extraction

### CST and candidates

- [x] Integrate and pin `tree-sitter-go`.
- [x] Parse complete Go files.
- [x] Locate raw string literal nodes with exact byte ranges.
- [x] Cover const declarations.
- [x] Cover var declarations.
- [x] Cover regular assignments.
- [x] Cover short assignments.
- [x] Reject ordinary non-SQL strings cheaply.
- [x] Accept explicit SQL markers before prefix classification.
- [x] Validate complete SQL through the PostgreSQL parser.
- [x] Reject incomplete fragments.
- [x] Keep interpreted strings disabled.

### Directives

- [x] Go file-level `semblock:file-ignore`.
- [x] Declaration-level `semblock:ignore`.
- [x] Explicit `semblock:sql`.
- [x] JetBrains `language=SQL`.
- [x] Structural comment-to-declaration attachment.
- [x] Diagnostics for misplaced/ambiguous directives.

### Rewrite safety

- [x] Format each eligible literal independently.
- [x] Abort the whole Go file when a mandatory literal fails.
- [x] Replace spans from end to start.
- [x] Preserve backticks and correct host indentation.
- [x] Reparse the complete resulting Go source.
- [x] Atomically replace only after validation.
- [x] Test multiple literals and atomic rollback.
- [x] Test malformed SQL and malformed Go.
- [x] Test full Go reparse after rewrite.
- [x] Batch 5 full tests and self-review.
- [x] Dead-code removal, docs, checklist, and commit.

Batch 5 MVP evidence:

- `tree-sitter 0.26.11` and `tree-sitter-go 0.25.0` locate source-owned raw
  literal and comment byte ranges.
- Directive attachment is resolved against supported CST owners, never by
  scanning Go strings as plain source text.
- `tests/cli_mvp.rs` covers automatic and explicit SQL, all four supported
  owner shapes, non-SQL and incomplete strings, every directive, malformed Go,
  malformed embedded SQL, whole-file rollback, indentation, and host reparse.
- Interpreted strings remain explicitly disabled and produce a diagnostic when
  targeted by an explicit SQL marker.

## Batch 6 — Performance and release polish

- [ ] Establish correctness-preserving benchmarks.
- [ ] Benchmark large project traversal.
- [ ] Benchmark large SQL and many Go literals.
- [ ] Avoid parsing ignored files.
- [ ] Measure before introducing parallelism.
- [ ] Add bounded `--jobs` parallel processing only if useful.
- [ ] Prove deterministic output and diagnostics across job counts.
- [ ] Define supported platforms and MSRV.
- [x] Add CI matrix for Rust 1.88 and current stable on Ubuntu 24.04.
- [ ] Add release profiles and reproducible release procedure.
- [ ] Produce release binaries/checksums.
- [ ] Document installation.
- [ ] Document shell and CI integration.
- [ ] Document stdin/editor integration.
- [ ] Batch 6 full tests, benchmarks, and self-review.
- [ ] Dead-code removal, docs, checklist, and commit.

## Batch 7 — Thin IDE adapters

- [ ] Stabilize formatter/stdin protocol first.
- [ ] VS Code thin adapter or task wrapper.
- [ ] IDEA external tool/file watcher.
- [ ] No IDE-local formatter engine.
- [ ] Adapter integration fixtures.
- [ ] Batch 7 tests and self-review.
- [ ] Docs, checklist, and commit.

## Per-batch self-review template

- [ ] Semantic changes are restricted to the documented lexical allowance.
- [ ] Original and formatted supported inputs parse.
- [ ] Idempotence passes.
- [ ] Comments remain attached and unchanged.
- [ ] Authored logical groups remain stable where required.
- [ ] Hard width is respected at every available safe boundary.
- [ ] Errors are explicit and actionable.
- [ ] No file can be partially rewritten.
- [ ] Dependencies are necessary, licensed, pinned appropriately, and documented.
- [ ] Architecture boundaries remain one-way.
- [ ] Unsupported syntax is not advertised.
- [ ] Dead code and obsolete compatibility paths are removed.
