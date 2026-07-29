# PL/pgSQL and dollar-quoted coverage — durable checklist

Status: **Batch P3 complete; Batch P4 pending**

This workstream completes the remaining Batch 3 routine-body tranche after the
PostgreSQL scalar-expression coverage. Update this file before every checkpoint
commit. A checked item requires focused tests and the batch self-review defined
in `AGENTS.md`.

## Decisions

- The pinned `pg_query 6.1.1` `parse_plpgsql` API is the syntax source of truth.
- Supported roots are anonymous `DO`, PL/pgSQL functions, and procedures.
- Dollar-quote delimiters are preserved exactly, including custom tags.
- Routine bodies use root indentation independent from the outer SQL header.
- Unsupported procedural nodes fail closed and preserve the complete source.
- Nested SQL uses the existing formatter facade and safety gates.
- No dependency is added.

## Batch P0 — Plan and baseline

- [x] Continue from expression-coverage head `94cf2b9fceb992a6f57603658de28fa3d38668c2`.
- [x] Create branch `agent/plpgsql-dollar-quoted-bodies`.
- [x] Read repository instructions, ownership/validation architecture, and style examples.
- [x] Verify `pg_query::parse_plpgsql` and characterize its JSON node shapes.
- [x] Record bounded support and fail-closed neighbors.

## Batch P1 — Parser-backed capability model

- [x] Add a dedicated procedural module without weakening ordinary SQL validation.
- [x] Validate outer routine language/body cardinality and exact dollar delimiters.
- [x] Parse the complete routine through `parse_plpgsql`.
- [x] Validate a closed set of supported procedural node names.
- [x] Reject unsupported nodes, non-PL/pgSQL roots, SQL bodies, and compact bodies.
- [x] Reparse output with both PostgreSQL parsers.

## Batch P2 — Nested body rendering and safety

- [x] Render `DECLARE`, `BEGIN`, `END`, assignments, `PERFORM`, and returns.
- [x] Format accepted embedded SQL through the existing formatter.
- [x] Render `IF` / `ELSIF` / `ELSE` and exception handlers.
- [x] Separate multiple exception handlers with blank lines.
- [x] Preserve custom dollar tags and body-root indentation.
- [x] Require PL/pgSQL structural equivalence and idempotence.
- [x] Add golden fixtures for DO, function, procedure, declarations, SQL, IF, and exceptions.
- [x] Add unsupported fixtures for loops, non-PL/pgSQL roots, and compact bodies.
- [x] Run focused tests and self-review.

## Batch P3 — CLI and host-language integration

- [x] Add malformed and unsupported project fixtures proving whole-project atomicity.
- [x] Add a realistic Go raw-string PL/pgSQL routine golden fixture.
- [x] Verify `gofmt`, offline `go test ./...`, custom dollar tags, and CRLF.
- [x] Add explicit CLI diagnostic assertions.
- [x] Run focused tests and self-review.

## Batch P4 — Reconciliation and final gate

- [ ] Mark PL/pgSQL blocks and dollar quoting complete in the main checklist.
- [ ] Document the routine-body support boundary.
- [ ] Run formatting, Clippy, all tests, Rustdoc, and `git diff --check`.
- [ ] Perform final architecture, safety, comment, dependency, and dead-code review.
- [ ] Commit and publish or prepare a verified bundle.
