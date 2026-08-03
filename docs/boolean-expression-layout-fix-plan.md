# Boolean-expression layout regression plan

## Summary

The regression affects two layers:

- Existing predicate owners such as `WHERE`, `HAVING`, and `JOIN ... ON`
  expand Boolean expressions, but mishandle compact child groups and closing
  parenthesis indentation.
- Value contexts such as `SELECT`, `RETURNING`, assignments, `VALUES`, `CASE`,
  and function arguments do not consistently expose their Boolean expressions
  to the shared planner.

Implement one context-independent Boolean-expression layout model covering
every already-supported expression owner. Do not broaden PostgreSQL syntax
support or change public APIs.

## Implementation changes

- Add a minimal anonymized CTE regression using `bb_*` identifiers, two
  `OR EXISTS` branches, nested predicates, and standalone comments.
- Introduce typed owned-expression ranges for predicate clauses, result-list
  items, assignment right-hand sides, `VALUES` items, `CASE` expressions, and
  function arguments.
- Build ranges only inside AST-validated statement, query, and list ownership;
  never scan the document globally for Boolean connectors.
- Plan Boolean groups recursively: expand mixed or independently complex
  groups, retain short cohesive child groups inline, break only connectors
  owned by an expanded group, and align closing parentheses with their group.
- Preserve comment attachment and use bound expression starts to keep syntax
  following standalone comments at its owner indentation.
- Do not change the shared `LayoutPlan::break_before` arbitration.
- Record the ownership decision in formatter design, architecture, extension
  guidance, and the durable implementation checklist.

## Test plan

- Cover `WHERE`, `HAVING`, `JOIN ... ON`, and representative DML/DDL predicate
  owners.
- Cover `SELECT`, `RETURNING`, assignments, `VALUES`, `CASE`, and Boolean
  function arguments.
- Require multiple `OR EXISTS` connectors to begin continuation lines, nested
  subqueries to align with their connector, and short `AND` groups to remain
  compact.
- Preserve standalone and inline comments, default `<>`, semantic equivalence,
  protected tokens, idempotence, hard-width safety, and clean
  `check(format(source))`.
- Prove ownership disagreement fails closed.
- Run focused tests and the complete repository quality gate, then review
  GitNexus `detect_changes` output before committing.

## Risk and assumptions

- Query and predicate binding are high-risk integration points and require
  broad existing-query regression coverage.
- Only already-supported PostgreSQL shapes receive additional layout
  ownership; unsupported neighbors remain unchanged with
  `syntax.unsupported`.
- Core specification 1.0.1 does not alter Boolean behavior. Its unrelated
  `CREATE VIEW` changes are outside this batch.
- A fresh Windows build requires LLVM/libclang or an equivalent build
  environment for the pinned `pg_query` dependency.

## Follow-up compactness refinement

The first implementation exposed two additional over-expansion regressions:

- an INSERT target list inside a commented CTE measured compact width from the
  CTE body span instead of the local `INSERT` token;
- predicate owners expanded every `AND` or `OR`, even when a same-precedence
  two-condition predicate was short and cohesive.

The architecture already contains the necessary ownership and planner
boundaries, so no ownership-IR redesign is required. Measure INSERT target-list
width from `InsertBlock::body_start`. Apply the shared mixed/nested/width rule
to predicates while retaining authored predicate boundaries, and let query
planning expand a nested query when its containing Boolean range is expanded.
Regression tests must keep the local predicates compact without collapsing the
surrounding mixed `OR EXISTS` structure.
