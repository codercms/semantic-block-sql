# PL/pgSQL and dollar-quoted coverage — durable checklist

Status: **Batch P0 complete; Batch P1 pending**

This workstream completes the remaining Batch 3 routine-body tranche after the
PostgreSQL scalar-expression coverage. Update this file before every checkpoint
commit. A checked item requires focused tests and the batch self-review defined
in `AGENTS.md`.

## Decisions

- The authoritative layout remains
  `docs/semantic-block-sql-fmt-check-core-spec.md` and the repository SQL skill.
- The pinned `pg_query 6.1.1` `parse_plpgsql` API is the syntax source of truth for
  PL/pgSQL bodies; body support must not be inferred only from keyword scans.
- Outer PostgreSQL statements are still parsed by the ordinary PostgreSQL parser.
- The initial supported roots are:
  - anonymous `DO` blocks;
  - `CREATE [OR REPLACE] FUNCTION ... LANGUAGE plpgsql`;
  - `CREATE [OR REPLACE] PROCEDURE ... LANGUAGE plpgsql`.
- The initial supported procedural nodes are deliberately bounded to:
  - block declarations and nested blocks;
  - SQL statements already accepted by the regular formatter;
  - assignment, `RETURN`, `RETURN NEXT`, `PERFORM`, `RAISE`, and `GET DIAGNOSTICS`;
  - `IF` / `ELSIF` / `ELSE`;
  - exception handlers with one or more conditions.
- Unsupported procedural nodes fail closed with `syntax.unsupported` and preserve
  the complete source byte-for-byte. Initial unsupported neighbors include loops,
  cursor control, dynamic `EXECUTE`, transaction control, `CASE` statements,
  `FOREACH`, and labeled control flow.
- Dollar-quote delimiters are preserved exactly, including custom tags. The body
  root is not indented relative to the outer routine header.
- Body comments and string literals must remain byte-identical and attached to the
  same statement. No comment-free AST rendering is allowed to discard source
  comments.
- Nested SQL uses the existing formatter facade and safety gates. Procedural
  expression text is parsed with the PostgreSQL PL/pgSQL expression mode exposed
  by the pinned backend where available; otherwise the tranche remains
  unsupported rather than using an unvalidated rewrite.
- No dependency addition is permitted.

## Batch P0 — Plan and baseline

- [x] Continue from expression-coverage head
  `94cf2b9fceb992a6f57603658de28fa3d38668c2`.
- [x] Create branch `agent/plpgsql-dollar-quoted-bodies`.
- [x] Read repository instructions, formatter ownership/validation architecture,
  current Batch 3 checklist, and the style-guide routine examples.
- [x] Verify the backend exposes `pg_query::parse_plpgsql` returning raw JSON.
- [x] Record the bounded node support and fail-closed neighbors.
- [x] Commit Batch P0 before behavior changes.

## Batch P1 — Parser-backed capability model

- [ ] Add a dedicated procedural module without weakening ordinary statement
  validation.
- [ ] Extract outer routine kind, language, body option, and exact dollar-quote
  delimiter from the PostgreSQL AST/source.
- [ ] Parse the complete routine through `parse_plpgsql`.
- [ ] Convert supported JSON nodes into a closed internal procedural sum type.
- [ ] Reject unsupported nodes with a stable feature name and source-preserving
  diagnostic.
- [ ] Validate routine language/body cardinality and reject SQL-language routines,
  string-literal bodies, multiple body options, and transformed/internal forms.
- [ ] Add parser/capability unit tests and Batch P1 self-review.
- [ ] Commit Batch P1.

## Batch P2 — Nested body rendering and safety

- [ ] Render `DECLARE`, `BEGIN`, nested blocks, and `END` with four-space nesting.
- [ ] Format accepted embedded SQL through the existing SQL formatter.
- [ ] Render assignment, `RETURN`, `RETURN NEXT`, `PERFORM`, `RAISE`, and
  diagnostics statements from parser-backed nodes.
- [ ] Render `IF` / `ELSIF` / `ELSE` blocks.
- [ ] Render `EXCEPTION` handlers with blank lines between multiple handlers.
- [ ] Preserve comments, blank-line boundaries, string literals, custom dollar
  tags, and body-root indentation.
- [ ] Reparse the complete formatted routine with both PostgreSQL parsers.
- [ ] Require routine idempotence and structural equivalence of both outer SQL and
  normalized PL/pgSQL JSON.
- [ ] Add golden fixtures for DO, function, procedure, nested IF, declarations,
  SQL statements, and multiple exception handlers.
- [ ] Run focused tests and Batch P2 self-review.
- [ ] Commit Batch P2.

## Batch P3 — Failure neighbors and project integration

- [ ] Add unchanged unsupported fixtures for loops, dynamic EXECUTE, cursors,
  transaction control, procedural CASE, labels, and non-plpgsql routines.
- [ ] Add malformed-body and malformed-outer-statement fixtures.
- [ ] Verify unsupported or malformed routines prevent every project write.
- [ ] Add a realistic Go raw-string routine fixture to the host-language golden
  project and compile it with `gofmt` and `go test ./...`.
- [ ] Verify CRLF and custom dollar tags through CLI formatting.
- [ ] Run focused tests and Batch P3 self-review.
- [ ] Commit Batch P3.

## Batch P4 — Reconciliation and final gate

- [ ] Mark PL/pgSQL blocks and dollar quoting complete in
  `docs/implementation-checklist.md`.
- [ ] Document the routine-body support boundary in formatter design docs.
- [ ] Run `cargo fmt --all -- --check`.
- [ ] Run `cargo clippy --locked --all-targets -- -D warnings`.
- [ ] Run `cargo test --locked --all-targets`.
- [ ] Run `cargo doc --locked --no-deps`.
- [ ] Run `git diff --check`.
- [ ] Perform final semantic, architecture, parser-boundary, comment, diagnostics,
  dependency, and dead-code self-review.
- [ ] Commit the final checkpoint and push the branch.
