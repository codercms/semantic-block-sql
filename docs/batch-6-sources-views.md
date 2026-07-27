# Batch 6 — relation sources and views

This tranche extends the hardened ownership architecture across richer DML
relation sources and PostgreSQL view definitions. It does not introduce a
parallel formatter path: every new form is validated into `StatementSpec`,
bound into `StatementLayout`, planned with shared query/predicate/list
primitives, and accepted only after the existing semantic-safety gates pass.

## Supported relation-source shapes

The shared `RelationListSpec` validator and `RelationSourceBlock` binder are used
by:

- `UPDATE ... FROM`;
- `DELETE ... USING`;
- `MERGE ... USING`.

A source list may contain:

- multiple ordinary relations;
- INNER, LEFT, RIGHT, FULL, CROSS, and NATURAL joins;
- join predicates using `ON` or `USING`;
- parenthesized join trees;
- a SELECT-derived table;
- one simple set-returning function, including `LATERAL` and
  `WITH ORDINALITY`.

Each top-level source item is recorded as `Relation`, `Subquery`, `Function`, or
`Join`. Every recursive join also records its type (`INNER`, `LEFT`, `RIGHT`, or
`FULL`) and constraint mode (`ON`, `USING`, `NATURAL`, or cross join), including
the `USING` column count. The token binder must reproduce those AST-derived
capabilities. Disagreement is an ownership safety failure.

Nested SELECTs in source items reuse ordinary `QueryBlock` ownership. This also
covers a derived table nested inside a joined source item. MERGE locates its own
final match `ON` independently of any `ON` predicates owned by its source joins.

The following remain unsupported and byte-identical:

- `ROWS FROM` and function column-definition lists;
- TABLESAMPLE;
- source aliases with column alias lists;
- `JOIN ... USING (...) AS alias`;
- data-modifying or other `WITH` queries inside derived sources;
- unreviewed relation-source AST nodes.

## `CREATE VIEW`

The owned subset supports:

- `OR REPLACE`;
- an optional column alias list;
- view options in `WITH (...)`;
- a SELECT query body;
- default/cascaded, explicit `CASCADED`, and `LOCAL` check options.

The view suffix is excluded from query-clause ownership, so `WITH ... CHECK
OPTION` cannot be interpreted as part of the SELECT body.

## `CREATE MATERIALIZED VIEW`

The owned subset supports:

- `IF NOT EXISTS`;
- an optional column alias list;
- an access method with `USING`;
- storage options in `WITH (...)`;
- `TABLESPACE`;
- a SELECT query body;
- explicit `WITH DATA`, explicit `WITH NO DATA`, and the default omitted data
  clause.

The data-population suffix is likewise outside query ownership.

The following remain unsupported:

- view or materialized-view queries with `WITH`;
- non-SELECT query bodies;
- transformed/internal PostgreSQL view fields;
- `CREATE TABLE ... AS`, which has distinct ownership and policy.

## Layout decisions

- Multiple comma-separated DML sources expand one item per line.
- JOIN clauses retain their source ownership and predicates.
- Derived SELECT wrappers expand structurally when their query expands.
- View `AS` and the query body begin on separate owner lines.
- Materialized-view storage clauses form separate owner lines when present.
- Authored comments remain attached to their source item, predicate, or query
  expression.

## Verification

`tests/batch6_sources_views.rs` covers:

- multiple UPDATE and DELETE sources;
- joined sources and predicate ownership;
- derived and function sources;
- derived sources nested inside joins;
- MERGE with derived and joined sources;
- view aliases, options, and check modes;
- materialized-view storage and all population modes;
- comment attachment;
- adjacent unsupported forms.

Every positive fixture requires golden output, PostgreSQL AST equivalence,
idempotence, clean `check` output, and no warnings. The complete repository gate
contains 117 passing tests for this checkpoint.
