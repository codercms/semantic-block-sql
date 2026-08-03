# Durable implementation checklist

Status: **Runnable CLI PoC complete; PostgreSQL statement coverage expanding**

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

- [x] `INSERT` with VALUES, source SELECT, DEFAULT VALUES, OVERRIDING, and RETURNING.
- [x] Simple and independently complex `VALUES` rows.
- [x] `ON CONFLICT` target predicate.
- [x] `ON CONFLICT DO UPDATE` action predicate.
- [x] `UPDATE ... FROM` with multiple, joined, derived, and function sources.
- [x] `DELETE ... USING` with multiple, joined, derived, and function sources.
- [x] `RETURNING` for fixture-backed `INSERT`.
- [x] Compact and expanded bounded `MERGE` branches.
- [x] `UNION`, `UNION ALL`, `INTERSECT`, and `EXCEPT`.
- [x] Lateral derived-table and function sources.
- [x] Grouping, `HAVING`, ordering, and pagination clauses.
- [x] Window expressions and named windows.
- [x] `FILTER` and ordered aggregate clauses.
- [x] PostgreSQL casts, arrays, and JSON expressions.
- [x] Basic `CREATE TABLE` columns and table constraints.
- [x] Simple one-line `CREATE INDEX`.
- [x] Simple one-line partial index.
- [x] Complex multiline `CREATE INDEX`.
- [x] Multi-action `ALTER TABLE` with syntactic action groups.
- [x] `CREATE VIEW` and `CREATE MATERIALIZED VIEW` bounded subsets.
- [x] MERGE with multiple, joined, derived, and function sources.
- [x] Bounded parser-backed PL/pgSQL blocks.
- [x] Dollar-quoted PL/pgSQL bodies with exact tag preservation.
- [x] Comments for every currently claimed Batch 3 statement family.
- [x] Golden, parse, idempotence, and safety fixtures for every checked item.
- [x] Batch 3 full tests and self-review.
- [x] Dead-code removal, docs, checklist, and commit.

PL/pgSQL tranche evidence:

- `src/formatter/procedural.rs` keeps procedural parsing separate from ordinary
  statement ownership and uses both PostgreSQL parsers as safety gates;
- `tests/batch8_plpgsql.rs` covers DO, function, procedure, declarations, SQL,
  IF/ELSE, multiple exception handlers, comments, custom dollar tags,
  idempotence, and fail-closed neighboring nodes;
- `tests/go_project_integration.rs` proves Go-host formatting, CRLF preservation,
  offline Go compilation, and whole-project atomicity for unsupported routines;
- compact single-line bodies and unreviewed procedural node families remain
  explicitly unsupported rather than partially rewritten.

Expression tranche evidence:

- `tests/fixtures/batch7/casts-arrays.*.sql` and
  `tests/fixtures/batch7/json-operators.*.sql` require exact golden layout,
  structural equivalence, idempotence, clean `check`, literal/comment
  preservation, and bounded-width behavior;
- square brackets and slice colons are rendered as owned array punctuation while
  grammar colons nested inside array elements retain SQL/JSON spacing;
- cast type casing covers `::`, `CAST`, quoted and unquoted qualified names,
  type modifiers, array suffixes, and contextual multiword type components;
- every reviewed legacy PostgreSQL JSON operator is covered across SELECT,
  predicates, UPDATE assignments, INSERT VALUES, and RETURNING;
- simple fixture-backed `JSON_OBJECT` and `JSON_ARRAY` remain supported, while
  unreviewed SQL/JSON root families return unchanged source with
  `syntax.unsupported`;
- the detailed durable record is
  `docs/postgresql-expression-coverage-checklist.md`.

Values, windows, lateral, and DDL tranche evidence:

- top-level VALUES owns row boundaries and preserves authored grouping;
- QueryBlock owns wrapper parentheses for nested and lateral SELECT sources;
- WindowBlock owns OVER, WITHIN GROUP, and named WINDOW bodies;
- CREATE TABLE validates and binds columns versus table constraints;
- CREATE INDEX owns keys, INCLUDE, storage parameters, TABLESPACE, and WHERE;
- ALTER TABLE owns action ranges, AST-derived syntactic action groups, and
  relation-level `SET (...)` / `RESET (...)` option lists;
- `tests/batch5_values_windows.rs` and `tests/batch5_ddl.rs` require golden
  output, semantic equivalence, idempotence, clean check output, exact comment
  preservation, fail-safe neighboring variants, authored relation-option
  groups, hard-width option splitting, and WHERE-equivalent predicate wrapping
  for AST-counted CREATE/ALTER TABLE CHECK constraints;
- `tests/batch5_ddl.rs` and `tests/batch14_utilities.rs` require
  document-relative fatal line diagnostics across ordinary statement
  reconstruction and recursive `COPY ... FROM STDIN` payload boundaries.
- `tests/batch2_core_layout.rs` requires indivisible-width diagnostics to map
  the causing output token back to its source line/range and rejects a short
  indivisible token as an excuse for an otherwise breakable over-hard line.
- authored blank lines between complete top-level statements are preserved.



Relation sources and views tranche evidence:

- UPDATE, DELETE, and MERGE share `RelationListSpec` AST capabilities and
  `RelationSourceBlock` token ownership;
- ordinary relations, multiple comma items, bounded joins, SELECT-derived
  tables, and simple set-returning functions are covered;
- source JOIN types and ON/USING/NATURAL/CROSS modes are capability-checked;
- parenthesized join trees are owned at their actual delimiter depth;
- source JOIN predicates remain distinct from UPDATE/DELETE WHERE and MERGE ON;
- CREATE VIEW owns aliases, options, SELECT body, and check mode;
- CREATE MATERIALIZED VIEW owns aliases, access method, options, tablespace,
  SELECT body, and explicit or omitted population mode;
- unsupported ROWS FROM, TABLESAMPLE, alias column lists, derived WITH queries,
  and CREATE TABLE AS remain byte-identical;
- `tests/batch6_sources_views.rs` requires golden output, AST equivalence,
  idempotence, clean check output, comment preservation, and fail-safe neighbors;
- the complete gate passes 117 tests.


## Batch 4 — Project CLI

Batch 4 and the raw-string subset of Batch 5 were implemented together as one
runnable MVP slice after Batch 2. Batch 3 is now proceeding incrementally behind
fixture-backed AST support gates; CLI capabilities do not imply syntax support.

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
- [x] Keep interpreted strings disabled in the historical raw-string MVP; superseded by the completed Go string-expression tranche below.

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

## Core specification v1 reconciliation

### Durable inputs and precedence

- [x] Preserve the 2026-07-26 core specification and latest skill ZIP with
  SHA-256 provenance.
- [x] Install the latest skill content as monotonic repository version `2.0.0`.
- [x] Make the core specification authoritative for machine `fmt` / `check`.
- [x] Record conflicts with the pre-spec implementation in the formatter design.

### Required implementation work

- [x] Replace optional indentation/group-preservation switches with mandatory
  four-space and authored-boundary behavior.
- [x] Add semicolon and not-equal policies with specification defaults.
- [x] Implement the exact casing whitelist and contextual `INTERVAL`.
- [x] Introduce rule-level diagnostics and a shared `fmt` / `check` analysis.
- [x] Return unchanged source plus diagnostics for parse, scan, and formatter
  safety failures.
- [x] Detect unsupported syntax explicitly and return `syntax.unsupported`.
- [x] Preserve final-newline presence.
- [ ] Preserve every existing compliant alternative layout.
- [ ] Add acceptance fixtures for every mandatory rule and statement family.
- [ ] Complete the reconciliation self-review and checkpoint.

Lexical-policy checkpoint evidence:

- `tests/spec_v1_lexical_policies.rs` covers the exact built-in whitelist,
  grammar-backed lowercase function names, contextual `INTERVAL`, type-modifier
  spacing, all semicolon policies, default/opt-in not-equal behavior, and final
  newline preservation.
- `tests/cli_mvp.rs` proves strict `[format]` configuration and end-to-end policy
  application through the CLI.
- The checkpoint passes formatting, Clippy with warnings denied, all 47 tests,
  documentation, and `git diff --check` in the packaged offline environment.

Diagnostic-model checkpoint evidence:

- `tests/spec_v1_diagnostics.rs` covers rule IDs, severities, source ranges,
  fail-safe parse results, policy-specific diagnostics, clean formatted output,
  tokenless whitespace, and allowed hard-width warnings.
- `tests/source_diagnostics.rs` covers SQL directive range shifting, CRLF byte
  offset restoration, and conservative Go raw-literal attribution.
- `semblock check` renders shared core diagnostics instead of filename-only
  `Would reformat` output, while `fmt` and `diff` surface warnings without
  reporting errors they successfully fix.
- A planner regression found during this batch now preserves a one-line construct
  when an indivisible token would remain over-hard after expansion.
- The checkpoint passes formatting, Clippy with warnings denied, all 56 tests,
  documentation, and `git diff --check` in the packaged offline environment.

Mandatory-layout checkpoint evidence:

- `FormatOptions` and `[layout]` expose only the configurable soft and hard line
  widths; obsolete indentation and preservation keys are rejected.
- `tests/batch2_core_layout.rs` requires four-space nesting and mandatory authored
  list, blank-line, and comment boundaries.
- `tests/cli_mvp.rs` proves the strict configuration rejection end to end.

One-line list expansion checkpoint evidence:

- A one-line list remains compact while it is short and simple.
- Once expansion is required, every ungrouped item receives its own line instead
  of being greedily repacked to the soft width.
- Authored multiline groups retain their existing grouping and are split only at
  safe item boundaries when the hard width requires it.
- Updated SELECT and function-argument fixtures cover the policy.

INSERT / VALUES / RETURNING checkpoint evidence:

- PostgreSQL AST classification admits only `INSERT ... VALUES ... RETURNING`
  without `WITH`, `OVERRIDING`, `ON CONFLICT`, `DEFAULT VALUES`, or a source
  query.
- `tests/batch3_insert.rs` covers compact INSERT, absent target-column lists,
  authored multirow VALUES, independently expanded complex rows, width-driven
  one-item-per-line column and RETURNING lists, exact comment preservation,
  structural equivalence, idempotence, clean `check`, and unsupported variants.
- `tests/cli_mvp.rs` continues to prove whole-project write atomicity using an
  unsupported DELETE statement as statement coverage expands.
- The checkpoint passes formatting, Clippy with warnings denied, all 67 tests,
  documentation, and `git diff --check` in the packaged offline environment.

ON CONFLICT checkpoint evidence:

- PostgreSQL AST classification admits fixture-backed `DO NOTHING`, named or
  inferred conflict targets, target predicates, and `DO UPDATE` assignments.
- The planner distinguishes the conflict-target predicate from the later update
  action `WHERE`, keeps `DO UPDATE`, `SET`, and action predicates in separate
  ownership tiers, and preserves compact `DO NOTHING` when readable.
- `EXCLUDED` is normalized only inside the owned `DO UPDATE` action; an ordinary identifier or target-table alias named `excluded` remains lowercase elsewhere.
- Inline comma-adjacent comments remain attached to the preceding assignment;
  later layout passes cannot move an authored inline comment to a standalone
  line.
- `tests/batch3_on_conflict.rs` covers compact and expanded forms, named
  constraints, both predicate owners, exact comments, structural equivalence,
  idempotence, clean formatted `check`, and `layout.on_conflict` diagnostics.

UPDATE checkpoint evidence:

- PostgreSQL AST classification admits a target relation, simple named
  assignments, zero or one plain FROM relation, optional `WHERE`, and
  `RETURNING` expressions.
- Compact single-assignment updates remain inline. Authored, width-driven,
  FROM-backed, or boolean-complex updates expand `SET` to one assignment per
  line and keep subsequent clauses at statement scope.
- `WITH`, `ONLY`, multi-column and subscripted targets, multiple or joined FROM
  sources, and UPDATE subqueries remain unchanged with `syntax.unsupported`.
- `tests/batch3_update.rs` covers compact and expanded forms, exact inline
  comments, structural equivalence, idempotence, clean formatted `check`,
  fail-safe unsupported variants, and `layout.update_set` diagnostics.
- The checkpoint passes formatting, Clippy with warnings denied, all 77 tests,
  documentation, and `git diff --check` in the packaged offline environment.

DELETE checkpoint evidence:

- PostgreSQL AST classification admits a target relation, zero or one plain
  `USING` relation, optional `WHERE`, and `RETURNING` expressions.
- Compact DELETE statements remain inline. Authored, USING-backed,
  width-driven, or boolean-complex statements place `USING`, `WHERE`, and
  `RETURNING` at statement scope without adding a connector-only indentation
  tier.
- `WITH`, `ONLY`, multiple or joined USING sources, derived sources, and DELETE
  subqueries remain unchanged with `syntax.unsupported`.
- `tests/batch3_delete.rs` covers compact and expanded forms, exact inline
  comments, structural equivalence, idempotence, clean formatted `check`, and
  fail-safe unsupported variants.

Unsupported-syntax checkpoint evidence:

- PostgreSQL AST classification admits only fixture-backed statement and SELECT
  shapes before the token planner runs.
- Unsupported DML, DDL, `MERGE`, routines, unowned SELECT clauses, general set
  operations, advanced aggregate/window forms, and lateral sources return the
  original source with `syntax.unsupported`.
- `tests/spec_v1_unsupported.rs` proves unchanged fail-safe results and preserves
  the supported recursive CTE `UNION ALL` fixture.
- `tests/cli_mvp.rs` proves an unsupported project file prevents every planned
  write.
- The checkpoint passes formatting, Clippy with warnings denied, all 60 tests,
  documentation, and `git diff --check` in the packaged offline environment.

Layout-IR consolidation evidence:

- `structure::TokenStructure` computes token depths and matching parentheses
  once for the complete formatting pass.
- `layout_ir::LayoutDocument` binds the AST-owned source statements into one
  exhaustive `StatementLayout` sum type and shared `QueryBlock`, `WithBlock`,
  `QueryClauses`, and `PredicateBlock` records.
- SELECT-list, query-clause, CTE, boolean, INSERT, UPDATE, and DELETE planners no
  longer rediscover ownership independently across the whole token stream.
- The migration is behavior-preserving: the complete existing suite remains
  green and new unit tests cover structural indexing plus query/predicate
  binding.

Ownership-IR checkpoint evidence:

- `validation::parse_supported_postgresql` returns one `SupportedDocument`
  containing an exhaustive statement specification and PostgreSQL `RawStmt`
  byte span for each fixture-backed top-level statement.
- `ownership::bind_token_statements` maps those AST-owned spans to token-indexed
  statement values; the later hardening checkpoint replaced these with
  capability-bearing `StatementSpec` and half-open `StatementTokens`.
- INSERT, UPDATE, and DELETE planners dispatch only from owned statement spans;
  they no longer scan the complete document for DML keywords.
- Missing expected clause ownership is an internal `format.safety_failure`, not
  a silently skipped planner.
- `docs/formatter-architecture.md` records the implemented structs, functions,
  safety pipeline, extension protocol, and forward-compatibility policy.
- Existing supported output remains unchanged and the complete suite contains
  82 passing tests, including an ownership binding unit test.

Query, INSERT-variant, and MERGE tranche evidence:

- `layout_ir::LayoutDocument` remains the single token-ownership binder. New
  syntax extends the closed statement/query IR rather than adding document-wide
  keyword discovery.
- INSERT now owns VALUES, source SELECT, DEFAULT VALUES, both OVERRIDING forms,
  ON CONFLICT, RETURNING, and shared WITH definitions.
- Query ownership covers DISTINCT, GROUP BY, HAVING, ORDER BY,
  LIMIT/OFFSET/FETCH, and general UNION / INTERSECT / EXCEPT branch boundaries.
- UPDATE and DELETE reuse the same `WithBlock` implementation for SELECT-backed
  CTEs; data-modifying CTEs remain fail-safe unsupported.
- MERGE adds one exhaustive `StatementSpec::Merge` and
  `StatementLayout::Merge`, with `MergeBlock`, `MergeBranch`, and `MergeAction`
  ownership. Branch actions reuse the existing assignment, delimited-list,
  predicate, comment, casing, and writer machinery.
- The MERGE support boundary currently owns a plain target and source relation,
  a join predicate, MATCHED / NOT MATCHED BY SOURCE / NOT MATCHED BY TARGET
  branches, optional conditions, DELETE, UPDATE SET, INSERT VALUES with
  OVERRIDING, DO NOTHING, WITH, and RETURNING. Derived or joined sources remain
  unchanged with `syntax.unsupported`.
- `tests/batch4_query_and_insert.rs` and `tests/batch4_merge.rs` cover golden
  output, semantic equivalence, idempotence, clean check results, exact comments,
  and adjacent unsupported variants.
- The checkpoint passes formatting, Clippy with warnings denied, all 96 tests,
  documentation, and `git diff --check` in the packaged offline environment.


## Architecture hardening checkpoint

- [x] Replace family-only `StatementKind` with capability-bearing
  `StatementSpec` variants.
- [x] Require DML and MERGE binders to verify clause presence, modes,
  cardinalities, and branch actions against the AST-validated shape.
- [x] Replace mixed inclusive/exclusive statement token bounds with half-open
  `TokenRange` plus a separate terminal semicolon.
- [x] Remove pointer-address allowlists from nested SELECT/VALUES validation.
- [x] Split layout binding into document, statement, and query modules.
- [x] Split planning into orchestration, statement, list, and rendering modules.
- [x] Separate structural equivalence from support classification.
- [x] Add a deliberate validator/binder disagreement regression test.
- [x] Document architecture and the compiler-guided PostgreSQL extension path.
- [x] Complete full quality gate and self-review.
- [x] Commit and package durable checkpoint.

Architecture-hardening evidence:

- `StatementSpec` now carries the exact AST-proven shape for SELECT, INSERT,
  UPDATE, DELETE, and MERGE; every field is consumed by token binding.
- `StatementTokens` uses a half-open `TokenRange` and a separate optional
  terminal semicolon.
- Family binders compare clause presence, enum modes, list cardinalities,
  branch actions, and set-operation counts against the validated shape.
- Nested SELECT/VALUES support is structural and deterministic; parser object
  addresses are no longer used as node identity.
- Layout ownership, statement binding, query binding, statement planning, list
  planning, rendering, and semantic equivalence are split into focused modules.
- All planners share one immutable `PlanningContext`; the formatter contains no
  `clippy::too_many_arguments` suppressions.
- The deliberate validator/binder mismatch test fails closed with an ownership
  safety diagnostic.
- The complete gate passes formatting, Clippy with warnings denied, all 98
  tests, Rustdoc, and `git diff --check` in the packaged offline environment.

## Expanded PostgreSQL coverage tranche

- [x] Common migration utilities: DROP, TRUNCATE, GRANT/REVOKE, COMMENT ON,
  enum/composite types, domains, sequences, triggers, and policies.
- [x] SELECT INTO, all row-lock strengths/options, data-modifying CTEs,
  SEARCH/CYCLE, and reviewed DML subqueries.
- [x] ROWS FROM, TABLESAMPLE, alias column/definition lists, and derived WITH
  relation sources.
- [x] Partitioned/inherited/typed CREATE TABLE forms and reviewed storage,
  access-method, tablespace, on-commit, and partition-bound clauses.
- [x] PL/pgSQL loops, FOREACH, procedural CASE, dynamic EXECUTE, cursor control,
  and reviewed EXIT/CONTINUE forms.
- [x] Realistic Go raw-string integration and whole-project atomic-failure
  coverage.
- [x] Golden output, PostgreSQL equivalence, idempotence, comments/protected
  literals, diagnostics, and fail-closed neighbors for every added family.

Evidence:

- `tests/batch9_migrations.rs` and `tests/fixtures/batch9/` cover common migration
  statements and adjacent unsupported shapes;
- `tests/batch10_query_dml.rs` covers INTO, locks, CTE extensions, and subqueries;
  it also proves that leading statement comments remain attached while WITH
  ownership is bound from the first non-comment syntax token, and that nested
  UPDATE ownership plus predicate-subquery indentation remain canonical inside
  a data-modifying CTE, while short `SET` and `RETURNING` lists remain compact
  when surrounding statement layout expands;
- `tests/batch3_update.rs` proves long ungrouped UPDATE lists split at safe item
  boundaries while authored `SET` and `RETURNING` groups remain intact when
  each group fits the hard limit;
- `tests/batch11_relations_tables.rs` covers advanced relation sources and table
  or partition DDL;
- `tests/batch12_plpgsql_control.rs` and `tests/fixtures/batch12/` cover procedural
  control flow, dynamic execution, cursors, comments, and protected strings;
- `tests/go_project_integration.rs` compiles representative migration/query/
  procedural raw strings with `gofmt` and offline `go test ./...`, and proves an
  unsupported procedural node prevents every project write;
- the durable implementation record is
  `docs/expanded-postgresql-coverage-checklist.md`.
- the complete offline gate passes formatting, Clippy with warnings denied, all
  170 tests, Rustdoc, and `git diff --check`.

## Boolean-expression ownership regression batch

- [x] Save the approved implementation and test plan in
  `docs/boolean-expression-layout-fix-plan.md`.
- [x] Add the anonymized `bb_*` CTE regression with multiple `OR EXISTS`
  branches and standalone comments.
- [x] Derive typed expression ranges from existing predicate, query, DML,
  VALUES, CASE, and function-list ownership.
- [x] Reuse one Boolean planner across SELECT, WHERE, HAVING, JOIN ON,
  RETURNING, assignment, VALUES, CASE, and function-argument contexts.
- [x] Keep short child `AND` groups compact and align expanded group and
  subquery closing parentheses.
- [x] Preserve grammar attachment for assignment values and CASE branches.
- [x] Classify mixed-Boolean list items before planning so formatting remains
  idempotent on the first pass.
- [x] Preserve default `<>`, comments, protected tokens, source order, and
  fail-closed unsupported behavior.
- [x] Cover semantic equivalence, idempotence, and clean `check` results in
  `tests/batch10_query_dml.rs`.
- [x] Complete formatting, Clippy, full tests, Rustdoc, `git diff --check`, and
  GitNexus change-scope review before commit.

## Compact INSERT-list and predicate refinement batch

- [x] Add a commented, anonymized data-modifying CTE regression whose short
  INSERT target list must remain inline.
- [x] Measure INSERT target-list width from the local `InsertBlock::body_start`
  rather than leading comments in the CTE statement span.
- [x] Add short same-precedence `JOIN ON`, `WHERE`, and `HAVING` regressions.
- [x] Preserve authored predicate boundaries while avoiding expansion solely
  because one `AND` or `OR` is present.
- [x] Keep nested queries expanded when their containing mixed Boolean range is
  expanded, without forcing a short nested predicate onto multiple lines.
- [x] Preserve mixed-Boolean, `OR EXISTS`, comment, equivalence, idempotence,
  and clean-check coverage in the focused formatter suites.
- [x] Complete formatting, Clippy, full tests, Rustdoc, `git diff --check`, and
  GitNexus change-scope review before commit.

## Statement-granular safety and FROM ownership regression batch

- [x] Add failing fixtures for SELECT and UPDATE uses of
  `IS [NOT] DISTINCT FROM`, including UPDATE FROM and WHERE positions.
- [x] Add a failing multi-branch set-operation fixture with branch-owned FROM
  clauses, named windows, and a final ORDER BY.
- [x] Bind FROM from typed statement ownership and exclude the distinctness
  operator token sequence.
- [x] Treat set-operation root FROM ownership separately from lexical query
  branches.
- [x] Preserve only a failed parser-proven statement under the default policy,
  diagnose it with its starting line, and continue formatting siblings.
- [x] Preserve complete-document behavior for untrusted parse/split failures
  and strict policy.
- [x] Cover CLI statement-level writes and strict Go static-concatenation
  atomicity.
- [x] Verify the reported real SQL file formats without an ownership failure.
- [x] Complete formatting, Clippy, full tests, Rustdoc, `git diff --check`, and
  GitNexus change-scope review before commit.

## Batch 7 — Performance and release polish

- [ ] Establish correctness-preserving benchmarks.
- [ ] Benchmark large project traversal.
- [ ] Benchmark large SQL and many Go literals.
- [ ] Avoid parsing ignored files.
- [ ] Measure before introducing parallelism.
- [x] Add bounded `--jobs` parallel processing only if useful.
- [x] Prove deterministic output and diagnostics across job counts.
- [ ] Define supported platforms and MSRV.
- [x] Add CI matrix for Rust 1.88 and current stable on Ubuntu 24.04.
- [ ] Add release profiles and reproducible release procedure.
- [ ] Produce release binaries/checksums.
- [ ] Document installation.
- [ ] Document shell and CI integration.
- [ ] Document stdin/editor integration.
- [ ] Batch 7 full tests, benchmarks, and self-review.
- [x] Dead-code removal, docs, checklist, and commit.

PR #7 Git-aware CLI hardening evidence:

- staged `check` and `diff` plan from UTF-8 stage-0 index blobs rather than
  reading worktree files;
- staged `fmt` compares every selected index blob with the worktree before
  planning, rejects partial staging with exit class 4, formats only the
  worktree, and never changes the index;
- `--changed-since` covers committed, staged, unstaged, and untracked live
  paths relative to the merge base;
- ordinary and Git-selected discovery share one configured `ignore::WalkBuilder`
  and Git candidates remain subject to nested `.semblockignore`, Git ignore,
  hidden-path, language, and Go rules;
- the builder's incremental ignore matcher classifies staged index paths whose
  worktree leaf is absent, so staged check/diff still inspect their blobs while
  ignored absent paths remain excluded before staged-fmt preflight;
- invocation-scoped Rayon planning collects in deterministic input order and
  completes project-wide preflight before writes;
- `tests/cli_workflows.rs` covers index/worktree divergence, staged-fmt safety,
  changed-since selection, invalid references, nested ignores, and deterministic
  parallel error selection.

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

## CLI diagnostic source locations

- [x] Reproduce the raw-byte-offset-only warning and error output.
- [x] Render one-based `line:column` before the half-open UTF-8 byte range.
- [x] Cover multibyte columns, unsupported warnings, strict errors, stdin, and
  CRLF input.
- [x] Complete formatting, Clippy, full tests, Rustdoc, `git diff --check`, and
  GitNexus change-scope review before commit.

## Go integration hardening workstream

The post-PR-7 fixture-backed Go integration work is tracked durably in
[`go-integration-hardening-checklist.md`](go-integration-hardening-checklist.md).
That checklist also records the newer embedded-SQL indentation decision from
style guide 1.0.1, which supersedes the historical MVP envelope behavior.

## Go string-expression and real-corpus tranche

- [x] Complete Go interpreted-string decode/encode round trip.
- [x] Expression-aware raw/interpreted/static-concatenation extraction.
- [x] Inline/nested calls, returns, assignments, composite values, `defer`, and `go` calls.
- [x] Runtime-value verification, full-file reparse, `gofmt`, compilation, and idempotence.
- [x] Expanded offline multi-package golden project.
- [x] Pinned, licensed, opt-in external corpus manifest and JSON reporting runner.
- [x] Normal CI remains network-independent.

The earlier MVP checkbox that kept interpreted strings disabled records the historical first slice only and is superseded by this completed tranche and `docs/real-world-readiness-checklist.md`.
