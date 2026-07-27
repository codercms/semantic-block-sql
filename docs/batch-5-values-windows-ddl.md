# Batch 5 values, windows, lateral sources, and DDL

Status: **complete checkpoint**

## Outcome

This checkpoint expands PostgreSQL coverage through the hardened ownership
architecture. Every new construct is admitted by AST validation, represented by
an explicit capability-bearing `StatementSpec` or query capability, rebound to
exact scanner tokens, and verified before layout. No document-wide fallback
scanner was added.

## Added query coverage

- top-level `VALUES` statements;
- aggregate `ORDER BY` and `WITHIN GROUP` ownership;
- `FILTER (WHERE ...)`;
- `OVER (...)`, including partitioning, ordering, and frame clauses;
- named `WINDOW` definitions;
- lateral derived tables;
- lateral function sources and `WITH ORDINALITY`;
- window expressions inside `INSERT ... SELECT`;
- owned wrapper parentheses for nested SELECT sources.

Long window bodies use owned internal breakpoints. A nested SELECT owns both its
query clauses and its wrapper parentheses, so the wrapper cannot remain inline
while the body expands independently.

## Added statement coverage

### `CREATE TABLE`

The current owned subset supports:

- `IF NOT EXISTS`;
- ordinary column definitions;
- column constraints represented by PostgreSQL's `ColumnDef`;
- table constraints;
- one element per line;
- a blank line between columns and table constraints;
- exact comment preservation at element boundaries.

The current subset intentionally rejects inheritance, partitioning, typed-table
forms, `CREATE TABLE ... AS`, and parser-transformed/internal nodes.

### `CREATE INDEX`

The current owned subset supports:

- `UNIQUE`;
- `CONCURRENTLY`;
- `IF NOT EXISTS`;
- access methods;
- expression and operator-class key items;
- ordering and NULL placement;
- `INCLUDE`;
- storage parameters in `WITH (...)`;
- `TABLESPACE`;
- partial-index `WHERE` predicates.

Simple indexes remain compact. Secondary clauses and long key lists expand at
owned clause or item boundaries.

### `ALTER TABLE`

The current subset supports user-authored action lists represented by reviewed
`AlterTableCmd` subtypes. Actions are one per line when expanded and receive
blank lines only when their syntactic action group changes. Internal PostgreSQL
rewrite commands remain unsupported.

## Safety boundary

The following neighboring shapes remain unchanged with
`syntax.unsupported`:

- `CREATE TABLE ... INHERITS`;
- partitioned tables;
- `LIKE ... INCLUDING ...` table elements;
- unowned ALTER forms such as relation/column rename;
- unknown future AST shapes;
- any validator/binder cardinality or capability disagreement.

The PostgreSQL grammar-version gate remains in force, so upgrading `pg_query`
cannot silently admit newer grammar fields.

## Additional fixes

- authored blank lines between top-level statements are preserved;
- function-call spacing in `INSERT ... SELECT` no longer confuses the target
  column list with later function parentheses;
- `WITH ORDINALITY` casing is contextual, while an alias named `ordinality`
  remains an identifier;
- DDL relation, constraint, and option parentheses use clause-appropriate
  spacing.

## Verification

The fixtures in `tests/batch5_values_windows.rs` and `tests/batch5_ddl.rs`
require:

- exact golden output;
- PostgreSQL AST equivalence;
- protected token preservation;
- idempotence;
- clean `check` output for canonical SQL;
- unchanged output for adjacent unsupported syntax;
- comment preservation at DDL and query boundaries.
