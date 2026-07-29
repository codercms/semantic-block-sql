# Expanded PostgreSQL coverage — durable checklist

Status: **Batch X3 complete; Batch X4 in progress**

This workstream is one PR-sized batch, split into durable internal checkpoints.
Every checked implementation item requires fixtures, parser-backed capability
validation, token-ownership agreement, semantic equivalence, idempotence, and a
batch self-review.

## Scope decisions

- Start from the merged `main` tree after PRs 9, 10, and 11.
- Preserve the closed AST capability model; no permissive "parsed means supported"
  fallback and no document-wide statement discovery.
- Migration statements that need no canonical multiline structure may share a
  closed `UtilityStatementSpec`, but every accepted AST family and option shape
  must remain explicit and fixture-backed.
- Existing formatter safety remains mandatory: unsupported neighbors return the
  original source with `syntax.unsupported`; one failing project input prevents
  every write.
- No dependency additions.

## Batch X0 — Baseline and characterization

- [x] Verify PRs 9, 10, and 11 are merged into `main`.
- [x] Synchronize the local working tree with the merged-main documentation tree.
- [x] Create branch `agent/expanded-postgresql-coverage`.
- [x] Read repository instructions, extension guide, architecture, and current
  formatter boundaries.
- [x] Record the complete requested scope and checkpoint order.
- [x] Characterize PostgreSQL AST and scanner shapes for every requested family.
- [x] Commit X0 before behavior changes.

## Batch X1 — Common migration SQL

- [x] `DROP` for reviewed object kinds and behavior modes.
- [x] `TRUNCATE` with multiple relations, identity mode, and cascade/restrict.
- [x] `GRANT` / `REVOKE` for object privileges and role membership.
- [x] `COMMENT ON` for reviewed object kinds.
- [x] `CREATE TYPE` composite and enum forms.
- [x] `CREATE DOMAIN` with defaults, nullability, collation, and constraints.
- [x] `CREATE SEQUENCE` options.
- [x] `CREATE TRIGGER` / constraint-trigger reviewed forms.
- [x] `CREATE POLICY` with roles, USING, and WITH CHECK.
- [x] Golden, comment, width, equivalence, idempotence, and fail-closed fixtures.
- [x] X1 self-review and checkpoint commit.

## Batch X2 — Query and DML variants

- [x] `SELECT ... INTO` capability and layout.
- [x] `FOR UPDATE`, `FOR NO KEY UPDATE`, `FOR SHARE`, and `FOR KEY SHARE`,
  including `OF`, `NOWAIT`, and `SKIP LOCKED`.
- [x] Data-modifying CTE bodies for reviewed INSERT/UPDATE/DELETE/MERGE shapes.
- [x] `SEARCH` and `CYCLE` CTE clauses.
- [x] Scalar/existence subqueries in UPDATE, DELETE, and MERGE expressions.
- [x] Preserve query-clause ownership and prevent DML predicate/subquery confusion.
- [x] Golden, comment, equivalence, idempotence, and fail-closed fixtures.
- [x] X2 self-review and checkpoint commit.

## Batch X3 — Relation sources and richer table DDL

- [x] `ROWS FROM` relation sources.
- [x] `TABLESAMPLE` with optional `REPEATABLE`.
- [x] Relation alias column-name and column-definition lists.
- [x] Derived relation sources whose query contains `WITH`.
- [x] Partitioned `CREATE TABLE`, partition bounds, and `PARTITION OF`.
- [x] Table inheritance.
- [x] Reviewed storage/access-method/tablespace/on-commit options.
- [x] Typed-table forms and reviewed typed-table column options.
- [x] Golden, comment, equivalence, idempotence, and fail-closed fixtures.
- [x] X3 self-review and checkpoint commit.

## Batch X4 — PL/pgSQL control-flow and dynamic execution

- [ ] Basic `LOOP`, `WHILE`, integer/query `FOR`, and `FOREACH`.
- [ ] Procedural `CASE` searched and simple forms.
- [ ] Dynamic `EXECUTE` with `INTO`, `STRICT`, and `USING`.
- [ ] Cursor declaration, `OPEN`, `FETCH`, `MOVE`, and `CLOSE`.
- [ ] `EXIT` / `CONTINUE` for reviewed unlabeled and labeled forms.
- [ ] Nested-body indentation, comments, exception blocks, custom dollar tags,
  parser-normalized equivalence, and idempotence.
- [ ] Unsupported adjacent procedural nodes remain byte-identical.
- [ ] X4 self-review and checkpoint commit.

## Batch X5 — Integration, documentation, and full gate

- [ ] Add representative successful Go raw-string migration/query/procedural
  fixtures and compile with `gofmt` plus offline `go test ./...`.
- [ ] Add project-wide atomic-failure fixtures for unsupported neighboring syntax.
- [ ] Update README, formatter design/architecture, extension guide, and main
  implementation checklist.
- [ ] Run `cargo fmt --all -- --check`.
- [ ] Run `cargo clippy --locked --all-targets -- -D warnings`.
- [ ] Run `cargo test --locked --all-targets`.
- [ ] Run `cargo doc --locked --no-deps`.
- [ ] Run `git diff --check`.
- [ ] Perform final semantic, ownership, comments, diagnostics, atomicity,
  dependency, and dead-code review.
- [ ] Commit final checkpoint, publish branch, and open one draft PR to `main`.
