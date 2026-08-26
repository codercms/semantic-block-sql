# PostgreSQL coverage

This document is the user-facing capability overview for `semblock`.

It describes **fixture-backed structural support**, not every grammar production accepted by PostgreSQL. The formatter is intentionally closed-world: parser-valid syntax is rewritten only after its relevant AST shape and presentation ownership have been reviewed.

For exact machine behavior, see the [core `fmt` / `check` specification](semantic-block-sql-fmt-check-core-spec.md). For implementation progress and engineering gates, see the [implementation checklist](implementation-checklist.md).

## Coverage model

There are three useful states:

| State | Behavior |
| --- | --- |
| Supported | Semblock owns and formats the reviewed construct. |
| Unsupported | The construct is valid PostgreSQL but outside the reviewed capability set; it remains byte-identical and reports `syntax.unsupported`. |
| Safety-skipped | The statement belongs to a supported family, but ownership or a safety invariant cannot be proven; it remains byte-identical and reports `format.statement_skipped`. |

Unsupported and safety-skipped statements are non-fatal by default. `--strict-unsupported` or `format.unsupported_policy = "error"` makes them fatal.

## Queries and expressions

Reviewed structural support includes:

- `SELECT` and `SELECT INTO`;
- ordinary and recursive CTEs;
- data-modifying CTEs;
- `SEARCH` and `CYCLE`;
- `UNION`, `INTERSECT`, and `EXCEPT`, including reviewed nested and parenthesized trees;
- scalar and predicate subqueries;
- result lists and function arguments;
- `WHERE` and `HAVING`;
- `GROUP BY` and `ORDER BY`;
- `LIMIT`, `OFFSET`, and `FETCH`;
- PostgreSQL row-lock strengths;
- `CASE`;
- filtered and ordered aggregates;
- named and inline window definitions;
- `VALUES`;
- reviewed subqueries inside DML expressions.

Authored multiline list and predicate groups, blank lines, and comment boundaries are preserved as structural presentation choices.

### Operators and PostgreSQL expressions

Fixture-backed coverage includes common PostgreSQL-specific operator families and contexts, including:

- JSON/JSONB navigation and containment;
- JSONPath `@?` and `@@`;
- `hstore`;
- arrays;
- ranges and multiranges;
- network operators;
- full-text search;
- regular expressions;
- `OPERATOR(schema.operator)`;
- `LIKE`, `ILIKE`, and `SIMILAR TO`;
- `COLLATE`;
- `AT TIME ZONE`;
- `IS UNKNOWN`;
- named function arguments.

The formatter preserves same-spelled identifiers when a token is not parser-owned grammar.

## Relation sources and joins

Reviewed relation-source support includes:

- single and multiple relations;
- `JOIN` / `INNER JOIN`;
- `LEFT`, `RIGHT`, and `FULL` joins, with and without `OUTER`;
- `CROSS JOIN`;
- `NATURAL` join variants;
- `ON` and `USING`;
- authored multiline `ON` and `USING` groups;
- derived queries;
- parenthesized join trees;
- `LATERAL`;
- function relation sources;
- `ROWS FROM`;
- `TABLESAMPLE`;
- reviewed alias column and column-definition lists;
- derived queries containing `WITH`.

Alias spelling is preserved from parser ownership even when PostgreSQL scans
the unquoted alias as a keyword. Fixture-backed coverage includes the complete
PostgreSQL 17 keyword table, every keyword legal as an unquoted relation alias,
all keywords as explicit output aliases, all bare labels as implicit output
aliases, every supported DML alias owner, join forms, CTEs, named windows,
views, relation alias columns, and function column definitions.

The same typed relation ownership is used in nested queries, CTEs, views, `INSERT ... SELECT`, DML `RETURNING`, `ON CONFLICT`, MERGE expressions, windows, and other reviewed query containers.

## Data modification

Reviewed support includes:

### INSERT

- `VALUES`;
- query sources;
- `DEFAULT VALUES`;
- `OVERRIDING`;
- `RETURNING`;
- `ON CONFLICT`, including reviewed target predicates and `DO UPDATE` assignments.

### UPDATE

- assignment lists;
- `FROM`;
- `WHERE`;
- `RETURNING`;
- reviewed subqueries in expressions.

### DELETE

- `USING`;
- `WHERE`;
- `RETURNING`;
- reviewed subqueries in expressions.

### MERGE

PostgreSQL 17 `MERGE` is supported for reviewed:

- source relations;
- `ON` predicates;
- matched and not-matched branches;
- update/insert/delete actions;
- reviewed branch expressions and subqueries.

## DDL

Fixture-backed support includes:

- `DROP`;
- `TRUNCATE`;
- object and role `GRANT` / `REVOKE`;
- `COMMENT ON`;
- enum and composite types;
- domains;
- sequences;
- triggers;
- row-security policies;
- `CREATE TABLE`;
- partitioned tables with range/list/hash partition keys;
- `PARTITION OF` with reviewed range/list/hash/default bounds;
- inheritance;
- typed tables;
- reviewed access method, storage, tablespace, and `ON COMMIT` options;
- feature-rich `CREATE INDEX`;
- multi-action `ALTER TABLE`;
- `CREATE VIEW`;
- `CREATE MATERIALIZED VIEW`, including reviewed storage options.

## Operational and migration statements

Reviewed support includes:

- `BEGIN` with reviewed transaction modes;
- unchained `COMMIT`, including `WORK` / `TRANSACTION` spellings;
- `COPY`, including protected `FROM STDIN` payloads;
- `CALL`;
- `EXPLAIN`;
- `VACUUM`;
- `ANALYZE`;
- `REFRESH MATERIALIZED VIEW`;
- `LISTEN`;
- `NOTIFY`;
- reviewed extension, schema, statistics, collation, and cast creation;
- reviewed `ALTER TYPE`, domain, policy, and rename forms.

## SQL routines and PL/pgSQL

Reviewed routine support includes SQL-standard `BEGIN ATOMIC` bodies within the supported body boundary and parser-backed PL/pgSQL formatting.

PL/pgSQL coverage includes:

- declarations;
- embedded SQL statements;
- `IF` / `ELSIF` / `ELSE`;
- exception handlers;
- loops;
- `FOREACH`;
- procedural `CASE`;
- dynamic `EXECUTE`;
- cursor operations;
- `ASSERT`;
- `RETURN QUERY`;
- reviewed `EXIT` and `CONTINUE`;
- compact bodies.

Routine grammar such as `FUNCTION` / `PROCEDURE`, `RETURNS`, and `LANGUAGE` is bound to parser-owned locations so same-spelled identifiers and user-defined types remain identifiers.

Optional built-in type-alias preferences apply within already-supported syntax
at parser-owned type locations and typed PL/pgSQL declarations. They do not
expand SQL syntax coverage: unsupported statements remain byte-identical, and
qualified, quoted, custom, and `float(p)` type names remain outside the rewrite.

## Known unsupported boundaries

The following valid PostgreSQL forms are deliberately preserved as unsupported today and have explicit boundary tests:

- `CREATE VIEW ... AS WITH ...`;
- `CREATE MATERIALIZED VIEW ... AS WITH ...`;
- subpartition declarations that combine `PARTITION OF` with another `PARTITION BY`;
- `CREATE TABLE ... LIKE ...`;
- `CREATE TABLE ... AS ...`;
- `CREATE PUBLICATION`;
- `CREATE SUBSCRIPTION`;
- `XMLTABLE`;
- `JSON_TABLE`;
- advanced SQL-standard JSON query/value/aggregate forms that are not in the reviewed expression subset;
- multi-statement SQL-standard `BEGIN ATOMIC` routine bodies;
- procedural transaction control.

This list highlights known high-value boundaries; it is not a promise that every PostgreSQL feature not listed here is already supported.

When adding syntax, semblock should continue to prefer an explicit capability record and regression fixture over permissive generic handling.

## Where to verify exact behavior

The most precise executable documentation is the test suite:

- `tests/coverage_layout_matrix.rs`
- `tests/coverage_joins.rs`
- `tests/coverage_operators.rs`
- `tests/coverage_set_operations.rs`
- `tests/coverage_support_boundaries.rs`
- the statement-family integration tests and realistic Go corpus

For contributor guidance on extending the capability set, see the [PostgreSQL extension guide](formatter-extension-guide.md).
