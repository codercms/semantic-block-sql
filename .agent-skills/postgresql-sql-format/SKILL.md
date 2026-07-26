---
name: postgresql-sql-format
description: Format PostgreSQL SQL and PL/pgSQL using the Semantic Block SQL style. Preserve semantics, order, literals, comments, aliases, and surrounding host-language or template code. Use for queries, migrations, DML, DDL, CTEs, MERGE, ON CONFLICT, functions, procedures, and embedded SQL. Do not optimize, fix, or redesign SQL unless separately requested.
compatibility: PostgreSQL; instruction-only Agent Skill for Claude Code and Codex.
metadata:
  version: "2.0.0"
  dialect: "postgresql"
  style: "semantic-block"
---

# Contract

Format only. Do not change SQL behavior.

For plain SQL, return formatted SQL only. For embedded or templated SQL, preserve all surrounding non-SQL code. Add explanation, review findings, or a diff only when requested.

If syntax is unfamiliar or the PostgreSQL dialect is uncertain, preserve it rather than guessing.

## Rule precedence

When rules conflict:

1. Preserve semantics, literals, identifiers, order, and comments.
2. Keep comments attached to the same syntax.
3. Expose nesting and mixed `AND` / `OR`.
4. Preserve authored logical groups.
5. Break safely before 160 characters.
6. Treat 120 characters as permission, not an obligation, to wrap.
7. Keep simple constructs compact.
8. Apply optional alignment last.

## Mandatory casing

Uppercase:

- SQL keywords and grammar constructs;
- `NULL`, `TRUE`, `FALSE`;
- `COUNT`, `SUM`, `AVG`, `MIN`, `MAX`;
- `COALESCE`, `NULLIF`, `GREATEST`, `LEAST`;
- `NOW`, `EXTRACT`;
- `CURRENT_DATE`, `CURRENT_TIME`, `CURRENT_TIMESTAMP`;
- `CURRENT_USER`, `CURRENT_ROLE`, `CURRENT_SCHEMA`, `SESSION_USER`;
- `LOCALTIME`, `LOCALTIMESTAMP`;
- `INTERVAL` only as a literal introducer: `INTERVAL '5 minutes'`.

Lowercase:

- every other unquoted function name;
- type names, including `interval` as a type.

Do not extend the uppercase whitelist. Preserve quoted names and literal contents exactly.

## Mandatory spacing

- Indent with four spaces; never tabs.
- Put one space around binary operators.
- Keep unary operators attached: `-1`.
- Put one space after an inline comma.
- Write casts as `value::type`.
- Write function calls as `name(...)`.
- Put a space before parentheses opened by SQL grammar: `IN (...)`, `ANY (...)`, `ALL (...)`, `EXISTS (...)`, `FILTER (...)`, `OVER (...)`, `WITHIN GROUP (...)`.
- Use trailing commas, never leading commas.
- Remove trailing whitespace.
- Preserve the context's terminal-semicolon policy; standalone SQL normally ends with `;`, embedded query strings may omit it.

## Layout

Keep a construct inline only when it is short, simple, and immediately readable.

Expand it when it:

- contains nested SQL;
- mixes `AND` and `OR`;
- has complex parentheses;
- contains an independently complex argument;
- exceeds 160 characters at a safe break point;
- becomes clearer after 120 characters;
- already contains authored groups or blank-line boundaries.

When expanded:

- indent contents one level below their owner;
- put `AND` and `OR` at the start of continuation lines;
- do not create lines containing only connector keywords such as `ON` or `THEN`;
- keep `JOIN ... ON` and `WHEN ... THEN UPDATE SET` on their owner lines;
- preserve precedence-significant parentheses.

## Lists and authored groups

For `SELECT`, `RETURNING`, `SET`, `VALUES`, `ORDER BY`, `GROUP BY`, function arguments, CTEs, and DDL action lists:

- preserve existing line groups while they fit within 160 characters;
- treat blank lines and comments as hard group boundaries;
- never merge across a blank line or comment;
- do not split an authored group only because it crosses 120;
- split safely if it crosses 160;
- if a long one-line list must expand and has no authored groups, place one item per line;
- never invent business-semantic groups.

Several layouts may be valid. Do not replace one valid authored grouping with another solely by preference.

## Comments

- Preserve comment text and syntax.
- A standalone comment belongs to the following syntax element.
- An inline comment belongs to the current expression.
- Do not move comments across expressions or groups.
- A comment may exceed 160 characters when moving it would change attachment.

## Boolean expressions and connectors

```sql
WHERE
    item.deleted_at IS NULL
    AND (
        item.title_rus IS NOT NULL
        OR item.title_orig IS NOT NULL
    )
```

```sql
LEFT JOIN source_links link ON
    link.item_id = item.id
    AND link.status = 'approved'
```

Do not emit an `ON`-only indentation tier.

## `CASE` and PL/pgSQL branches

`CASE` is an expression; short results stay after `THEN`:

```sql
CASE
    WHEN item.id IS NULL THEN 0
    ELSE 1
END
```

PL/pgSQL branches contain statements:

```sql
IF item.deleted_at IS NOT NULL THEN
    RETURN;
END IF;
```

```sql
EXCEPTION
    WHEN foreign_key_violation THEN
        RAISE NOTICE 'Item does not exist';

    WHEN unique_violation THEN
        RAISE NOTICE 'Item already exists';
END;
```

Align `EXCEPTION` with `BEGIN`. Indent each `WHEN` one level and its statements one additional level. Separate multiple handlers with a blank line.

## Statement summary

- CTEs: start each CTE on a new line after the previous `),`.
- Set operations: put blank lines around `UNION`, `INTERSECT`, and `EXCEPT`; preserve parentheses and precedence.
- `VALUES`: expand only complex rows; rows may use different layouts.
- `ON CONFLICT`: keep the target predicate attached to `ON CONFLICT`; separate `DO UPDATE` and its action `WHERE`.
- `UPDATE`: when `SET` expands, use one assignment per line.
- `DELETE`: apply the general compact-or-expanded rules.
- `MERGE`: separate `WHEN` branches with blank lines; keep actions on the `WHEN ... THEN` line.
- DDL: keep simple indexes compact; separate table columns from constraints; group `ALTER TABLE` actions with blank lines.
- Functions and procedures: separate signature, `RETURNS`, attributes, and body introducer. Indent `BEGIN ATOMIC` contents one level.
- Dollar-quoted PL/pgSQL: `DECLARE`, `BEGIN`, `EXCEPTION`, and `END` are at the body root, without an extra indent caused by `AS $$`.
- Embedded SQL: keep SQL-relative indentation independent from the host-language block; leading and trailing newlines are allowed.
- Templated SQL: format SQL regions without moving or rewriting template syntax.

For exact statement layouts, read [references/STYLE.md](references/STYLE.md). For compact examples, read [references/EXAMPLES.md](references/EXAMPLES.md).

## Semantic safety

Never:

- add or remove casts, columns, predicates, joins, CTEs, assignments, or branches;
- reorder any semantic element;
- change join types or query structure;
- rename or invent aliases;
- change literals or quoted identifiers;
- remove potentially meaningful parentheses;
- optimize or repair SQL;
- move a comment to another syntax element.

Prefer `!=` in new or already edited code, but do not rewrite `<>` solely for formatting.

Before returning, apply [references/CHECKLIST.md](references/CHECKLIST.md).
