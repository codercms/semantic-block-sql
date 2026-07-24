---
name: postgresql-sql-format
description: Use this skill to format or reformat PostgreSQL SQL and PL/pgSQL for readability and consistent structure without changing behavior. Apply to queries, CTEs, DML, DDL, MERGE, ON CONFLICT, window functions, set operations, migrations, and procedural blocks. Do not use it as a query optimizer or semantic rewriter unless the user separately requests review.
compatibility: PostgreSQL syntax. Instruction-only; no external tools required. Compatible with Agent Skills clients including Codex and Claude Code.
metadata:
  version: "1.1.0"
  dialect: "postgresql"
---

# PostgreSQL SQL formatting

Format PostgreSQL according to its syntax tree and logical structure. Optimize for human readability while preserving behavior.

## Workflow

1. Confirm the input is PostgreSQL SQL or PL/pgSQL. If the dialect is materially ambiguous, preserve unfamiliar syntax rather than rewriting it.
2. Identify the statement hierarchy, nested queries, clause arguments, boolean groups, and logical sections before changing whitespace.
3. Apply the core rules below.
4. For non-trivial statements, consult [references/STYLE.md](references/STYLE.md).
5. For unfamiliar statement shapes or formatting ambiguity, consult [references/EXAMPLES.md](references/EXAMPLES.md).
6. Perform the safety and consistency checks in [references/CHECKLIST.md](references/CHECKLIST.md).
7. Return formatted SQL only unless the user asks for explanation, review findings, or a diff.

## Core rules

### Casing

- Uppercase SQL keywords, keyword-like SQL constructs, and special values: `NULL`, `TRUE`, `FALSE`.
- Use lowercase function names and type names.
- Preserve quoted identifiers, string literals, comments, and dollar-quoted contents exactly.
- Preserve unquoted identifier spelling unless the user explicitly asks to normalize it.
- In PostgreSQL mode, prefer `!=` over `<>`; normalization from `<>` to `!=` is allowed.

### Indentation and spacing

- Use four spaces for every syntactic nesting level. Never use tabs.
- Use one space after commas and around binary operators.
- Do not add spaces around `::` casts or between a function name and `(`.
- Use a space before parentheses introduced by SQL operators or clauses, such as `IN (...)`, `ANY (...)`, `FILTER (...)`, and `OVER (...)`.
- Remove trailing whitespace.
- Put the semicolon at the end of the final clause, never on a separate line.

### Compact or expanded layout

Use 120 columns as the preferred width. A cohesive expression may exceed it when splitting would reduce readability.

Keep a construct inline when it is short, simple, and immediately understandable:

```sql
SELECT id, kp_id, imdb_id
FROM public.items
WHERE deleted_at IS NULL AND status = 'active'
ORDER BY created_at DESC, id;
```

Expand a construct when any of the following apply:

- it has many arguments;
- it exceeds the preferred width;
- it contains nested SQL;
- it mixes `AND` and `OR`;
- it contains complex parentheses;
- one argument is independently complex;
- its arguments belong to different logical groups;
- compact formatting obscures structure.

When expanded:

- keep the complete clause introducer on its owner line when possible, such as `JOIN ... ON` or `WHEN ... THEN UPDATE SET`;
- indent the clause contents one level below that owner line;
- do not create an extra indentation level for a line containing only `ON`, `THEN`, or another structural connector;
- use one syntactic argument or one logical argument group per line;
- put `AND` and `OR` at the beginning of continuation lines;
- preserve parentheses that expose boolean precedence;
- use trailing commas, never leading commas.

```sql
WHERE
    item.deleted_at IS NULL
    AND (
        item.title_rus IS NOT NULL
        OR item.title_orig IS NOT NULL
    )
```

### Logical groups and blank lines

Use blank lines to separate logical units, including:

- unrelated groups of selected expressions;
- unrelated join groups;
- set-operation branches;
- `MERGE` branches;
- recursive CTE anchor and recursive terms;
- DDL columns and table constraints;
- distinct `ALTER TABLE` action groups;
- procedural sections whose separation improves scanning.

Do not insert blank lines mechanically between every clause or expression.

### Clause arguments

Apply the same inline-or-expand decision to every argument-bearing construct:

- `SELECT`, `RETURNING`: result expressions or logical column groups;
- `WHERE`, `HAVING`, `ON`: predicates or parenthesized predicate groups;
- `ORDER BY`, `GROUP BY`, `PARTITION BY`: expressions;
- `SET`: assignments;
- `VALUES`: rows, then values inside each row;
- function calls: arguments or logical groups such as JSON key/value pairs;
- `WITH`: CTE definitions;
- `WINDOW`: named window definitions;
- `ALTER TABLE`: alteration actions;
- `MERGE`: `WHEN` branches and nested actions;
- `CREATE TABLE`: columns and constraints.

Related short arguments may share a line when the group remains easy to scan.

### Boolean expressions

Keep a short cohesive predicate group inline:

```sql
WHERE source.batch_id = $1::uuid AND source.validation_error IS NULL
```

Expand mixed or nested boolean logic:

```sql
AND (
    (entity_lock.entity_source = 'kp' AND entity_lock.entity_id = link.kp_id)
    OR (entity_lock.entity_source = 'imdb' AND entity_lock.entity_id = link.imdb_id)
)
```

Expand the inner groups further only when they become independently long or complex.

### `CASE`

Prefer compact branches inside a multiline `CASE`:

```sql
CASE
    WHEN item.id IS NULL THEN 0
    WHEN item.deleted_at IS NOT NULL THEN -1
    ELSE first_expression + second_expression
END
```

A short complete `CASE` may remain inline:

```sql
status = CASE WHEN source.approved THEN 'approved' ELSE 'rejected' END
```

Expand a branch only when its condition or result is independently complex.

### Aliases

- Use `AS` for result-column aliases.
- Omit `AS` for table and subquery aliases.

```sql
SELECT count(*) AS sessions
FROM public.items item
JOIN (...) source ON source.item_id = item.id;
```

### Alignment

Visual alignment of `AS`, `=`, types, or constraints is optional human-oriented polish, not a correctness rule.

Preserve sensible existing alignment or add it only within a short homogeneous block when it clearly improves readability. Do not align when it creates large whitespace gaps, exceeds the preferred width, crosses logical groups, or makes diffs noisy.

### `ON CONFLICT`

Prefer:

```sql
ON CONFLICT (version) DO UPDATE
SET
    config = EXCLUDED.config,
    is_active = EXCLUDED.is_active,
    updated_at = now()
```

For a partial conflict target:

```sql
ON CONFLICT (kp_id) WHERE kp_id IS NOT NULL
DO UPDATE
SET
    ...
```

If the target predicate is complex, expand and indent it beneath `ON CONFLICT`. A later top-level `WHERE` belongs to the update action.

### `MERGE`

Place each `WHEN` at statement scope and use blank lines between branches. Keep the action introducer on the `WHEN ... THEN` line to avoid a redundant indentation level:

```sql
WHEN MATCHED AND condition THEN UPDATE SET
    value = source.value

WHEN NOT MATCHED THEN INSERT (id, value)
    VALUES (source.id, source.value)
```

A branch without nested arguments stays fully inline:

```sql
WHEN MATCHED AND source.deleted_at IS NOT NULL THEN DELETE
```

### Set operations

Put blank lines around `UNION`, `UNION ALL`, `INTERSECT`, and `EXCEPT`. Preserve branch order and precedence exactly.

### DDL

- Keep simple DDL on one line when it fits comfortably.
- In `CREATE TABLE`, put one column or constraint per line and separate columns from table constraints with a blank line.
- In `ALTER TABLE`, indent each action and use blank lines between logical action groups.
- A simple index may stay on one line:

```sql
CREATE INDEX users_reg_date_idx ON users (reg_date);
CREATE INDEX users_reg_date_idx ON users (reg_date) WHERE (deleted_at IS NULL);
```

Expand an index only when its expressions, `INCLUDE`, predicate, options, or width benefit from it.

### PL/pgSQL

Format known PL/pgSQL structurally:

```sql
DO $$
    DECLARE
        value bigint;
    BEGIN
        ...
    END
$$;
```

Apply the ordinary SQL rules to SQL statements inside procedural blocks. Do not reinterpret arbitrary dollar-quoted strings whose language is unknown.

## Semantic safety

Formatting must not perform semantic rewriting, except for the explicitly allowed PostgreSQL lexical normalization from `<>` to `!=`.

Do not:

- add or remove casts;
- reorder expressions, predicates, joins, CTEs, assignments, or set-operation branches;
- change join types or query structure;
- rename or invent aliases;
- add or remove selected columns;
- optimize correlated subqueries or replace them with joins;
- change operators other than `<>` to `!=`;
- change literal contents;
- remove potentially meaningful parentheses;
- change comments or attach them to a different syntax node;
- apply query-performance recommendations unless the user asks for review.

If the SQL appears semantically suspicious, format it as written and report the concern separately only when review commentary is requested.
