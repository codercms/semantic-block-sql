# 0.3.0 formatter coverage and regression report

The migration coverage work adds **107 tests** relative to the main branch after
the authored-layout fix. The original 36 regression/desired-support cases and
all 22 follow-up migration cases are green. The complete local suite passes
**454 tests** across 57 all-target test invocations, including Go/Rust SQL parity.

Fixtures use invented schema, object, parameter, and literal names. Production
SQL and domains were not copied into the repository. Private audits ran on local
temporary copies; original source files were not rewritten.

## Coverage improvements

| Reviewed capability | Executable coverage |
| --- | --- |
| Nested SELECT, LATERAL, procedural CTEs, CTE-backed views/materialized views, wrapped set-operation owners | [Layout regressions](../tests/synthetic_layout_regressions.rs), [desired SQL coverage](../tests/desired_sql_coverage.rs) |
| Ordinary SET forms, aggregate definitions, index partition attachment, composite type fields | [Utility SQL coverage](../tests/utility_sql_coverage.rs) |
| SQL RETURN bodies, multi-statement atomic SELECT/DML/RETURN, dollar-quoted SQL bodies, routine options and signatures | [SQL routines](../tests/sql_standard_routine.rs), [desired SQL coverage](../tests/desired_sql_coverage.rs) |
| Procedural CTE/DML leaves, RAISE options, ELSIF, RETURN NEXT, assignment forms, optional final END semicolon, static maintenance statements | [Procedural SQL](../tests/procedural_sql_coverage.rs), [desired SQL coverage](../tests/desired_sql_coverage.rs) |
| Procedural SELECT/RETURNING INTO, STRICT, datatype references, and COMMIT/ROLLBACK chain modes | [Follow-up regressions](../tests/remaining_migration_regressions.rs), [procedural controls](../tests/batch12_plpgsql_control.rs) |
| Sequence/identity options, transition-table and long trigger headers, foreign-key actions | [Migration DDL](../tests/migration_ddl_coverage.rs), [follow-up regressions](../tests/remaining_migration_regressions.rs) |
| Standalone/derived/CTE-body VALUES; ORDER BY, LIMIT, OFFSET, FETCH; DML derived sources | [VALUES ownership](../tests/values_relation_coverage.rs) |
| Keyword/qualified/quoted function names, generated escape helpers, AS-less recordset aliases, array element groups, nested join predicates, CHECK/CASE/relation width budgets | [Follow-up regressions](../tests/remaining_migration_regressions.rs) |
| C/internal declarations, exact library/symbol literals, common options, comments, and long AS argument lists | [External routines](../tests/external_routine_coverage.rs) |

These are reviewed shapes, not blanket PostgreSQL grammar support. The
[coverage reference](sql-coverage.md) retains fixture-backed negative boundaries,
including advanced JSON/XML forms, unreviewed routine languages/body statements,
VALUES set-operation branches, and direct INSERT VALUES query suffixes.

## Fixed regressions

| Problem | Result and evidence |
| --- | --- |
| Authored multiline clauses or logical groups collapsed | SELECT/DML/DDL owners preserve compliant authored boundaries; [layout matrix](../tests/coverage_layout_matrix.rs). |
| Blank line after a comment or dump header removed | Shared emission retains hard blank boundaries; all **873** header gaps in the original private snapshot remain unchanged. |
| Nested query/CTE/relation ownership lost, or indentation increased inconsistently | Typed query/relation owners share contextual indentation and preserve their enclosing spans; layout and follow-up targets require idempotence. |
| Keyword-tokenized function names lost argument-list ownership | AST call-name roles distinguish explicit names and parser-generated helpers; commented calls and same-spelled aliases have controls. |
| Width ignored qualified call names, named CHECK prefixes, LATERAL definitions, arrays, or WHEN/THEN prefixes | Owner budgets include their line prefixes and nested groups; generic reductions assert the hard limit without splitting tokens. |
| Outer NOT EXISTS claimed an inner WHERE connector | Contained query connectors remain with their query owner; the former second-pass expansion is reproduced and fixed. |
| Procedural INTO treated as SQL table syntax | PL parser queries bind the elided target span; SQL is formatted separately and targets/comments are restored at their authored token boundary. |
| Datatype reference normalized as arithmetic/type text | Parser-recorded %TYPE/%ROWTYPE spelling remains intact; arithmetic percent expressions remain separate. |
| Warnings pointed to attached comments or stale pre-format lines | Unsupported/skipped diagnostics start at SQL syntax; fmt reports rewritten coordinates on every run; [CLI locations](../tests/cli_diagnostic_locations.rs). |
| Multiline routine/comment width warnings fell back to file line 1 | Parser-owned bodies map inner token identity and occurrence; repeated runs, type aliases, Unicode, CRLF, and ordinary dollar strings are covered by [source diagnostics](../tests/source_diagnostics.rs). |

## Validation and private audit

The initial independent migration audit recorded 85 unsupported diagnostics,
28 statement safety skips, and two fatal width failures. The completed audit
reports **zero unsupported diagnostics, zero safety skips, and zero fatal
formatting failures** for the supplied copies and the original Git snapshot.

All three audited copies pass `check` with exit code 0. A repeated `fmt` returns
exit code 0 and leaves every file byte-identical. Each schema representation
retains four legitimate indivisible-token width warnings, now attributed to the
correct token/line; the smaller query copy has none.

The complete engineering gate passes:

```text
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo doc --locked --no-deps
git diff --check
```

The implementation keeps parser validation, typed ownership, layout planning,
diagnostics, host extraction, and atomic rewriting separate. Structural and
protected-token equivalence and byte idempotence remain mandatory. No formatter
safety gate was weakened, and no dependencies or parser-backend revisions were
added. The application version is 0.3.0; the optimized Windows build is verified
separately from the test build.
