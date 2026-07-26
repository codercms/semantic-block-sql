# Batch 3 query and MERGE coverage

Status: **complete checkpoint**

## Outcome

This checkpoint finishes the ownership-IR architecture migration and exercises
it with a broader PostgreSQL syntax tranche. New syntax is added by extending
AST validation and owned layout records; no new document-wide keyword scanners
were introduced.

## Architecture

The formatter now has one ownership pipeline:

1. `validation::parse_supported_postgresql` classifies exact PostgreSQL AST
   shapes into the closed `StatementKind` sum type.
2. `ownership::SupportedDocument` records top-level source spans.
3. `structure::TokenStructure` computes token depth and matching delimiters once.
4. `layout_ir::LayoutDocument` binds statements, queries, clauses, predicates,
   CTEs, set operations, and MERGE branches to exact source tokens.
5. `semantic_block` planners consume only those owned records and reuse shared
   list, predicate, CASE, comment, width, casing, and writer logic.

Adding MERGE required one `StatementKind` variant, one validator, one
`StatementLayout` variant, one binder, and one planner. Existing statement
planners did not need to change their ownership models.

## Added syntax coverage

- INSERT source SELECT;
- INSERT DEFAULT VALUES;
- INSERT OVERRIDING SYSTEM/USER VALUE;
- SELECT-backed WITH on INSERT, UPDATE, and DELETE;
- DISTINCT;
- GROUP BY and HAVING;
- ORDER BY;
- LIMIT, OFFSET, and FETCH;
- UNION, UNION ALL, INTERSECT, and EXCEPT;
- bounded PostgreSQL 17 MERGE:
  - plain target and source relations;
  - USING ... ON predicate;
  - MATCHED, NOT MATCHED BY SOURCE, and NOT MATCHED BY TARGET;
  - optional branch conditions;
  - DELETE;
  - UPDATE SET;
  - INSERT (...) [OVERRIDING ...] VALUES (...);
  - DO NOTHING;
  - SELECT-backed WITH;
  - RETURNING.

## Deliberate fail-safe limits

The following still return unchanged source with `syntax.unsupported`:

- data-modifying CTEs;
- lateral, derived, joined, function, or multi-relation DML/MERGE sources not yet
  owned by a dedicated source IR;
- window expressions, named WINDOW clauses, FILTER, and ordered aggregates;
- top-level VALUES;
- DDL, routines, and PL/pgSQL;
- unknown future PostgreSQL AST shapes.

## Safety evidence

The fixtures require:

- exact PostgreSQL parse-tree equivalence;
- protected comment/literal/identifier preservation;
- idempotence;
- clean `check` results for canonical output;
- unchanged output for neighboring unsupported variants;
- whole-project no-partial-write behavior through the CLI suite.
