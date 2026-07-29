# Real-world formatter readiness — durable three-PR checklist

Status: **PR 1 planning complete; classifier refactor in progress**

This roadmap is intentionally split into three stacked pull requests. Each PR
must pass the complete repository quality gate and remain independently
reviewable. PR 2 is based on PR 1; PR 3 is based on PR 2.

## Locked product decisions

- Unsupported but valid PostgreSQL is **non-fatal by default**.
- Default behavior preserves each unsupported unit byte-for-byte, reports a
  `syntax.unsupported` warning, and continues formatting supported siblings.
- Strict handling is explicit through `unsupported_policy = "error"` and
  `--strict-unsupported`.
- Malformed SQL, malformed explicitly marked embedded SQL, semantic mismatch,
  protected-token changes, ownership failures, non-idempotence, and host-source
  reparse failures remain fatal.
- Formatting remains atomic per file. Strict mode keeps project-wide no-write
  preflight. Default mode may write files whose supported units were safely
  formatted while unsupported units remain untouched.
- Go interpreted strings are enabled by default after a proven decode/format/
  encode implementation exists.
- Multiline interpreted SQL converts to a raw literal when conversion is
  lossless; otherwise it remains interpreted with deterministic escaping.
- Compile-time concatenations made exclusively from string literals are eligible;
  expressions containing identifiers, calls, indexing, or other runtime values
  remain untouched.
- PL/pgSQL is refactored to a typed semantic IR plus a separate layout IR. The
  line-oriented renderer is removed rather than extended indefinitely.
- No generic "PostgreSQL parsed it, therefore it is supported" escape hatch is
  allowed. Every formatter-owned family remains AST-validated and fixture-backed.
- No new runtime dependency is added unless the existing parser/tooling cannot
  safely satisfy the requirement and the dependency is explicitly reviewed.

## PR topology

1. `agent/unsupported-classification-refactor` → `main`
2. `agent/plpgsql-ir-formatter` → PR 1 branch
3. `agent/go-string-coverage` → PR 2 branch

After an upstream PR is merged, rebase or retarget the next branch before merge.
Avoid squash-merging an intermediate stacked PR unless the downstream branch is
restacked afterward.

# PR 1 — Refactor support classification and unsupported policy

## C0 — Characterization and public model

- [x] Record the three-PR roadmap and locked decisions.
- [ ] Characterize current formatter, CLI, source, Go, and project-preflight error
  paths for unsupported syntax.
- [ ] Add `UnsupportedPolicy::{Skip, Error}` with default `Skip`.
- [ ] Add strict TOML key `format.unsupported_policy`.
- [ ] Add CLI override `--strict-unsupported` for fmt/check/diff and stdin flows.
- [ ] Document exit-code behavior for both policies.
- [ ] Commit C0.

## C1 — Statement-granular SQL classification

- [ ] Split a PostgreSQL document into parser-proven top-level statement units.
- [ ] Classify each unit as supported, unsupported, or fatal-invalid.
- [ ] Preserve inter-statement whitespace and comments exactly outside formatted
  unit spans.
- [ ] Format supported statements independently.
- [ ] Preserve unsupported statement bytes and emit warning diagnostics by default.
- [ ] Collect all unsupported diagnostics rather than stopping at the first.
- [ ] In strict mode, elevate unsupported diagnostics to errors and return the
  original complete source unchanged.
- [ ] Preserve semantic equivalence and idempotence checks per supported unit and
  for the reconstructed document.
- [ ] Add mixed supported/unsupported/malformed document fixtures.
- [ ] Commit C1.

## C2 — File/project and embedded-SQL policy propagation

- [ ] Propagate unsupported warnings through `.sql`, stdin, and Go-host call paths.
- [ ] Ensure one unsupported literal does not suppress formatting of supported
  literals in the same Go file.
- [ ] Keep malformed explicitly marked SQL fatal.
- [ ] Make auto-detected non-SQL/parse-failing candidates skippable without
  claiming that they are SQL.
- [ ] Default fmt writes safe supported changes while preserving unsupported units.
- [ ] Strict mode performs complete project preflight and writes nothing when any
  unsupported unit exists.
- [ ] Check/diff return differences independently from unsupported warnings;
  strict unsupported errors retain exit code 3.
- [ ] Add serial/parallel determinism and project atomicity tests for both policies.
- [ ] Commit C2.

## C3 — Utility statement expansion

- [ ] Support reviewed forms of `COPY`, including query sources and protected
  `FROM STDIN` payloads.
- [ ] Support `CALL`.
- [ ] Support `EXPLAIN` with options and a nested supported/unsupported statement.
- [ ] Support reviewed `VACUUM` and `ANALYZE` option/relation forms.
- [ ] Support `REFRESH MATERIALIZED VIEW`.
- [ ] Support `LISTEN` and `NOTIFY`.
- [ ] Support `CREATE EXTENSION`.
- [ ] Support reviewed `ALTER TYPE`, `ALTER DOMAIN`, and `ALTER POLICY` forms.
- [ ] Support `CREATE RULE` with nested actions.
- [ ] Support `CREATE STATISTICS`, `CREATE COLLATION`, and `CREATE CAST`.
- [ ] Characterize and include low-cost frequent neighbors such as `CREATE SCHEMA`,
  `ALTER SEQUENCE`, `ALTER INDEX`, `ALTER VIEW`, `ALTER MATERIALIZED VIEW`, and
  `ALTER TRIGGER` only when they fit the same closed strategy.
- [ ] Add dedicated layout IR where option lists or nested statements need more
  than token-normalization.
- [ ] Add golden, equivalence, idempotence, comment, payload-protection, and
  unsupported-neighbor fixtures for every family.
- [ ] Commit C3.

## C4 — PR 1 reconciliation

- [ ] Update core spec, design, architecture, extension guide, README, and CLI docs.
- [ ] Run formatter checks, Clippy with warnings denied, all tests, Rustdoc, and
  `git diff --check` using the supplied offline toolchain.
- [ ] Perform semantic, diagnostics, policy, atomicity, dependency, and dead-code
  self-review.
- [ ] Produce a verified PR 1 git bundle and SHA-256 file.
- [ ] Record Windows apply/push/PR instructions.

# PR 2 — PL/pgSQL semantic IR and formatter

## P0 — Typed parser model

- [ ] Replace JSON-name allowlisting as the primary renderer model with typed
  deserialization/adaptation into `PlpgsqlRoutine`, declarations, blocks,
  statements, branches, expressions, SQL units, cursors, and exception handlers.
- [ ] Preserve parser source locations and exact source spans for every owned node.
- [ ] Represent unmapped parser nodes as `Unsupported` only when a trustworthy
  source span exists; otherwise preserve the complete routine.
- [ ] Keep outer SQL routine extraction and exact dollar-tag ownership separate.
- [ ] Commit P0.

## P1 — Procedural token binding and layout IR

- [ ] Add procedural tokenization/binding independent from original source lines.
- [ ] Build a dedicated layout IR for sequences, declarations, branches, labels,
  nested blocks, embedded SQL, comments, and opaque unsupported spans.
- [ ] Preserve comment attachment, protected literals, authored blank-line groups,
  custom dollar tags, and CRLF.
- [ ] Remove the line-oriented frame renderer after parity fixtures pass.
- [ ] Commit P1.

## P2 — Formatter coverage

- [ ] Format compact and multi-statement single-line bodies.
- [ ] Format multiline declarations, assignments, conditions, and dynamic execute
  arguments structurally.
- [ ] Support `ASSERT`.
- [ ] Support `RETURN QUERY` and reviewed `RETURN QUERY EXECUTE` forms.
- [ ] Preserve unsupported procedural statements while formatting safely bound
  supported siblings under default policy.
- [ ] Strict policy makes unsupported routine nodes fatal without writes.
- [ ] Retain existing blocks, IF/CASE, loops, FOREACH, cursors, RAISE, diagnostics,
  exceptions, EXIT/CONTINUE, and dynamic EXECUTE behavior through the new IR.
- [ ] Add nested control-flow, comments, compact-body, equivalence, and idempotence
  fixtures.
- [ ] Commit P2.

## P3 — PR 2 reconciliation

- [ ] Add mixed ordinary SQL/routine and mixed supported/unsupported routine tests.
- [ ] Update documentation and remove obsolete renderer helpers.
- [ ] Run the complete offline quality gate and self-review.
- [ ] Produce a verified PR 2 git bundle and SHA-256 file.
- [ ] Record stacked Windows apply/push/PR instructions.

# PR 3 — Real Go string-expression coverage

## G0 — Go string codec and expression IR

- [ ] Add a complete interpreted-string decoder for Go escapes.
- [ ] Add deterministic interpreted-string encoding and round-trip tests against
  Go `strconv.Unquote` / `strconv.Quote` in the test toolchain.
- [ ] Add `GoStringExpression` IR for raw literals, interpreted literals, and
  static literal-only concatenations.
- [ ] Classify expression contexts directly rather than relying only on broad
  declaration owners.
- [ ] Commit G0.

## G1 — Real expression contexts

- [ ] Support declaration and assignment values.
- [ ] Support direct and nested function-call arguments.
- [ ] Support return values, standalone calls, `defer`, and `go` calls.
- [ ] Support struct/composite literal fields, map values, slice/array elements,
  and test-table entries.
- [ ] Exclude import paths, struct tags, build directives, runes, and non-expression
  strings structurally.
- [ ] Support literal-only static concatenation and preserve dynamic expressions.
- [ ] Make explicit SQL markers work on interpreted/static string expressions.
- [ ] Commit G1.

## G2 — Rewriting policy and safety

- [ ] Enable interpreted strings by default.
- [ ] Preserve one-line interpreted output when formatted SQL stays one line.
- [ ] Convert multiline interpreted SQL to raw strings when exact and legal.
- [ ] Fall back to deterministic interpreted escaping when SQL contains backticks,
  carriage returns, or another raw-string blocker.
- [ ] Decode replacements and verify exact runtime string value.
- [ ] Reparse the complete Go source, run `gofmt`, and require idempotence.
- [ ] Ensure unsupported SQL warnings do not suppress other literal replacements.
- [ ] Commit G2.

## G3 — Real-project corpus

- [ ] Expand the checked-in golden project with realistic repository, migration,
  test-table, pgx/database-sql/sqlx-like, nested-call, and inline-literal patterns.
- [ ] Add a pinned external-project corpus manifest and opt-in runner for several
  permissively licensed Go projects.
- [ ] Report candidate, formatted, unsupported, false-positive, build, and test
  outcomes.
- [ ] Require `gofmt`, selected `go test`, and semblock idempotence.
- [ ] Keep the normal offline CI independent from network corpus execution.
- [ ] Commit G3.

## G4 — PR 3 reconciliation

- [ ] Update README/config/docs and remove the interpreted-string MVP limitation.
- [ ] Run the complete offline quality gate and architecture/dead-code review.
- [ ] Produce a verified PR 3 git bundle and SHA-256 file.
- [ ] Record stacked Windows apply/push/PR instructions.
