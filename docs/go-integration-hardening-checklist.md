# Go integration hardening — durable checklist

Status: **Implementation complete; publication handoff in progress**

This workstream strengthens embedded-Go SQL support after PR 7. Update this
file before every checkpoint commit. A checked item requires focused tests and
the batch self-review described in `AGENTS.md`.

## Decisions

- The uploaded `semantic-block-sql-style-guide-ru-1.0.1.md` is the latest
  explicit project requirement for embedded SQL layout.
- Multiline Go raw-string SQL uses SQL-root indentation independent from the Go
  block. The opening and closing raw-string delimiters remain valid Go syntax;
  SQL lines are not re-indented to the declaration or call-site column.
- Golden fixture projects use adjacent `.go.expected` files.
- Go compilation fixtures use only the standard library and must not require
  network access.
- Host-language candidate detection stays structural and conservative. Raw
  literals participating in string concatenation are fragments and are skipped
  unless a future explicit fragment mode is designed.
- New owner contexts are added only with syntax-tree fixtures. Detection is not
  tied to `database/sql` method names.

## Batch G0 — Durable plan and baseline

- [x] Verify the uploaded bundle points at merged PR 7 commit
  `ac3ca9a7c85a1c644e034eff3da2a9d554db9275`.
- [x] Read `AGENTS.md`, the formatter design/architecture documents, the core
  specification, the repository SQL skill, and the current Go extractor.
- [x] Record the embedded-SQL indentation precedence decision.
- [x] Define implementation batches and acceptance gates.
- [x] Create branch `agent/go-golden-project-integration`.
- [x] Commit Batch G0 before behavior changes.

## Batch G1 — Golden project harness and SQL-root envelopes

- [x] Add a realistic multi-package Go fixture project with adjacent golden
  files.
- [x] Cover package-level const/var declarations, grouped declarations, local
  declarations, assignments, nested calls, comments, directives, unchanged
  non-SQL literals, and representative supported PostgreSQL statements.
- [x] Compare every generated `.go` file byte-for-byte with its golden file.
- [x] Verify `check --list-different`, `fmt`, clean `check`, and idempotence.
- [x] Verify `--jobs 1` and `--jobs 4` produce byte-identical trees.
- [x] Run `gofmt -l` and require no output.
- [x] Run `go test ./...` and require success.
- [x] Change multiline raw envelopes to SQL-root indentation and update focused
  legacy tests.
- [x] Preserve CRLF envelopes in a focused fixture.
- [x] Run focused tests and Batch G1 self-review.
- [x] Commit Batch G1.

## Batch G2 — Concatenated-fragment safety

- [x] Add a failing fixture for raw SQL-looking literals in Go binary string
  concatenation.
- [x] Classify raw-literal usage structurally.
- [x] Skip concatenated fragments during auto-detection without suppressing
  unrelated complete SQL literals in the same owner/file.
- [x] Define and test explicit-marker behavior for a concatenated expression.
- [x] Preserve project/file atomicity.
- [x] Run focused tests and Batch G2 self-review.
- [x] Commit Batch G2.

## Batch G3 — Direct return and expression-statement owners

- [x] Introduce a closed `GoSqlOwnerKind` model.
- [x] Support complete raw SQL literals nested in `return_statement`.
- [x] Support complete raw SQL literals nested in `expression_statement`.
- [x] Cover direct `return db.QueryContext(...)` and standalone
  `db.ExecContext(...)` in the golden project.
- [x] Keep `defer_statement` and `go_statement` unsupported unless separately
  fixture-backed.
- [x] Keep directive attachment deterministic and reject misplaced markers.
- [x] Run focused tests and Batch G3 self-review.
- [x] Commit Batch G3.

## Batch G4 — Failure projects, CI, and final gate

- [x] Add invalid-SQL, invalid-Go, and directive-error fixture projects.
- [x] Verify a failure prevents every project write.
- [x] Add explicit pinned Go setup to CI before Rust tests invoke Go tooling.
- [x] Document the host integration suite and supported owner boundary.
- [x] Run `cargo fmt --all -- --check`.
- [x] Run `cargo clippy --locked --all-targets -- -D warnings`.
- [x] Run `cargo test --locked --all-targets`.
- [x] Run `cargo doc --locked --no-deps`.
- [x] Run `git diff --check`.
- [x] Perform final dead-code and architecture self-review.
- [ ] Commit the final Batch G4 checkpoint and verify a portable git bundle.
- [ ] Push the branch and open a draft PR when authentication is available.

Final validation on 2026-07-29:

- Rust formatting passed.
- Clippy passed for all targets with warnings denied.
- All 138 Rust tests passed, including the Go project compilation suite.
- Rust documentation built successfully.
- `git diff --check` passed.
- Final review found no new dead code, database-library name coupling, or
  accidental support for `defer`/`go` statement owners.
