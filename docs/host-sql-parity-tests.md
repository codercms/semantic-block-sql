# Go and Rust SQL coverage parity

Go and Rust use the same PostgreSQL engine and capability boundary. Rust adds
host extraction, not a separate SQL dialect or layout. The parity suite in
`tests/host_sql_parity.rs` makes this contract executable using existing SQL:

- All 29 checked-in `*.input.sql` fixtures are formatted through both adapters.
- Complete SQL values in the realistic Go project are reused verbatim for Rust.
- Valid SQL literals are located structurally in the existing Rust integration
  test sources and deduplicated. The current corpus has 732 distinct values;
  input and expected SQL are both exercised. Malformed SQL and dynamic host
  expressions retain their separate host safety tests.
- Each SQL value is embedded in ordinary and raw Go/Rust literals, plus the
  direct SQL positions of `sqlx::query!` and `sqlx::query_as!`. Explicit markers
  allow comment-first queries without broadening automatic detection.
- Decoded results must equal the canonical engine's output, unsupported/skipped
  diagnostic rules and severities must match, unchanged SQL must retain its
  value, and second-pass output must be byte-identical.
- The existing high-risk unsupported-boundary cases also run under strict
  policy, requiring identical error diagnostics and unchanged SQL values.

The broad matrix uses default formatter settings. It removes surrounding
newlines/common host indentation before comparison; a separate test feeds the
original indented Go migration unchanged to both adapters. PostgreSQL structural
equivalence is checked for ordinary statements. Routines use exact canonical
output because the outer PostgreSQL comparator treats bodies as string values;
the canonical procedural formatter supplies its own safety checks.

## Complexity exercised identically

| SQL family | Existing shared cases |
| --- | --- |
| Queries | Nested subqueries/CTEs, recursive and data-modifying CTEs, joins/lateral sources, mixed Boolean precedence, CASE, aggregates, windows, grouping, set-operation precedence, sorting and locking |
| Expressions | Casts, JSON/JSONB and PostgreSQL operators, arrays, quoted identifiers, protected literal contents, placeholders, comments and authored groups |
| DML | INSERT/VALUES, ON CONFLICT, UPDATE FROM, DELETE USING, MERGE branches and RETURNING |
| DDL/utilities | Tables/constraints, partitions, indexes, views, types, ALTER/DROP, grants and the reviewed utility statements in the regression suite |
| Routines | SQL and PL/pgSQL functions/procedures, DO blocks, declarations, branches, loops, exceptions and dynamic EXECUTE |
| Safety boundaries | Existing valid but unsupported statements retain the same canonical opaque output and diagnostics in either host |

Presence in the corpus is not a claim that every form in a family is supported.
The engine's support/unsupported decision is compared as well as formatting;
the [SQL coverage reference](sql-coverage.md) remains authoritative.

## Permanent projects and real compilation

`tests/fixtures/rust-project/` mirrors the complete SQL corpus of
`tests/fixtures/go-project/` in a multi-module Cargo project with nine `.rs`
files with the same 32 complete SQL expressions as Go. Tests require identical decoded SQL multisets for inputs and independently
checked whole-file goldens. Serial/parallel CLI output, discovery, ignored files,
check/diff no-write behavior, idempotence and locked offline Cargo compilation
are checked. No extra dependency or database is needed.

`tests/fixtures/rust-sqlx-project/` separately pins real SQLx 0.8.6 and reuses Go
SQL for all six reviewed macros and a chained query function. Its opt-in test
compiles before and after formatting using metadata generated against real
PostgreSQL for both query spellings. It runs offline without a database once
its locked dependencies have been fetched. See the fixture README for commands.
