# Semantic Block SQL — `fmt` / `check` Core Specification

Version: 1.0

## 1. Scope

This specification defines only the formatting and style-checking behavior for PostgreSQL SQL and PL/pgSQL.

It does **not** define:

- CLI commands or flags;
- file discovery;
- ignore files;
- editor integration;
- SQL extraction from Go, Rust, PHP, or other languages;
- parser/library selection;
- performance architecture;
- query optimization or semantic linting.

The core operates on SQL text plus formatting configuration and returns formatted text and/or diagnostics.

## 2. Non-goals

The core must not:

- optimize SQL;
- repair invalid SQL;
- redesign query structure;
- add or remove casts, columns, predicates, joins, CTEs, assignments, aliases, or branches;
- reorder semantic elements;
- rename identifiers or aliases;
- change join types;
- rewrite subqueries as joins;
- change literals, quoted identifiers, comments, or template syntax;
- enforce SQL design, performance, or security practices.

## 3. Public behavior

Conceptual interface:

```text
format(source, config) -> FormatResult
check(source, config)  -> CheckResult
```

Suggested result model:

```text
FormatResult {
    output: string
    diagnostics: Diagnostic[]
    changed: bool
}

CheckResult {
    diagnostics: Diagnostic[]
    compliant: bool
}
```

The concrete programming-language API is out of scope.

## 4. Configuration

```text
soft_line_width = 120
hard_line_width = 160

semicolon_policy = preserve
not_equal_policy = preserve
syntax_diagnostics = parser_available
```

### 4.1 Line widths

Both limits must be configurable.

- `soft_line_width` is a readability hint, not a violation threshold.
- Crossing the soft limit does not make valid authored formatting non-compliant.
- `hard_line_width` is enforced only when a safe syntax boundary exists.
- Indivisible strings, quoted identifiers, URLs, comments, and tokens may exceed the hard limit.

Validation:

```text
soft_line_width > 0
hard_line_width >= soft_line_width
```

### 4.2 Semicolon policy

Supported values:

```text
preserve   # default
require
omit
```

Default behavior is `preserve`.

- `preserve`: do not add or remove terminal semicolons.
- `require`: add a semicolon only when the top-level statement boundary is unambiguous.
- `omit`: remove only the terminal semicolon of the formatted unit.

Embedded SQL normally uses `preserve`.

### 4.3 Not-equal policy

Supported values:

```text
preserve   # default
prefer_bang
```

- `preserve`: keep both `<>` and `!=` unchanged and emit no style diagnostic.
- `prefer_bang`: `check` reports `<>`; `fmt` may rewrite it to `!=`.

`<>` is valid PostgreSQL and must not be reported by default.

### 4.4 Syntax diagnostics

Syntax validation is not a style responsibility.

If the selected parser already exposes syntax errors with negligible extra complexity:

- return them as `syntax.*` diagnostics;
- do not classify them as style violations;
- do not format any part of a document whose top-level statement boundaries
  cannot be established reliably.

If reliable syntax diagnostics are unavailable, the core may return one generic parse-failure diagnostic.

## 5. Global invariants

### 5.1 Semantic preservation

Formatting must preserve:

- statement and expression order;
- operators, except configured `<>` normalization;
- aliases;
- casts;
- literals;
- quoted identifiers;
- placeholders;
- comments and their attachment;
- precedence-significant parentheses;
- template and host-language regions supplied as protected ranges.

### 5.2 Idempotence

```text
format(format(source, config).output, config).output
    == format(source, config).output
```

### 5.3 Fail-safe behavior

If the formatter cannot parse the document well enough to establish trusted
top-level statement spans, return the complete original source and a diagnostic.

After those spans are established, safety is statement-granular:

- preserve an unformattable statement byte-for-byte;
- emit `format.statement_skipped` for its complete source range;
- under the default skip policy, continue formatting independent sibling
  statements;
- under the strict policy, elevate skipped statements to errors and return the
  complete original document unchanged;
- never emit a partially formatted statement or perform a partial filesystem
  write.

### 5.4 Existing valid layout

More than one layout may be valid.

The formatter must preserve an existing authored layout when it:

- satisfies all mandatory rules;
- remains within the hard limit where safely breakable;
- keeps nesting and boolean precedence readable.

Do not replace one valid grouping with another solely to canonicalize appearance.

## 6. Rule precedence

When rules conflict:

1. Preserve semantics and literal contents.
2. Preserve comment attachment.
3. Expose nesting and mixed boolean precedence.
4. Preserve authored logical groups.
5. Respect the configured hard limit where safely breakable.
6. Use the soft limit as a readability signal.
7. Keep simple constructs compact.
8. Apply optional alignment last.

## 7. Mandatory casing

Uppercase:

- SQL keywords and grammar constructs;
- `NULL`, `TRUE`, `FALSE`;
- `COUNT`, `SUM`, `AVG`, `MIN`, `MAX`;
- `COALESCE`, `NULLIF`, `GREATEST`, `LEAST`;
- `NOW`, `EXTRACT`;
- `CURRENT_DATE`, `CURRENT_TIME`, `CURRENT_TIMESTAMP`;
- `CURRENT_USER`, `CURRENT_ROLE`, `CURRENT_SCHEMA`, `SESSION_USER`;
- `LOCALTIME`, `LOCALTIMESTAMP`;
- `INTERVAL` only as a literal introducer.

Lowercase:

- every other unquoted function name;
- type names, including `interval` as a type.

Examples:

```sql
SELECT
    COUNT(*) AS item_count,
    COALESCE(MAX(score), 0) AS max_score,
    NOW() + INTERVAL '5 minutes' AS expires_at,
    date_trunc('day', created_at) AS created_day
FROM public.items;
```

Do not extend the uppercase whitelist implicitly.

Preserve quoted names and all literal contents exactly.

## 8. Mandatory spacing

- Four spaces per real nesting level.
- No tabs.
- One space around binary operators.
- Unary operators remain attached: `-1`.
- One space after an inline comma.
- Casts: `value::type`.
- Function calls: `name(...)`.
- SQL grammar parentheses: `IN (...)`, `ANY (...)`, `ALL (...)`, `EXISTS (...)`, `FILTER (...)`, `OVER (...)`, `WITHIN GROUP (...)`.
- Trailing commas, never leading commas.
- No trailing whitespace.

## 9. Compact and expanded layout

Keep a construct inline only when it is short, simple, and immediately readable.

Expand when it:

- contains nested SQL;
- mixes `AND` and `OR`;
- contains complex parentheses;
- has an independently complex argument;
- exceeds the hard limit at a safe break point;
- becomes materially clearer after the soft limit;
- already contains authored line or blank-line groups.

When expanded:

- indent contents one level below their owner;
- place `AND` and `OR` at the start of continuation lines;
- preserve precedence-significant parentheses;
- do not create connector-only lines such as a line containing only `ON` or `THEN`;
- keep `JOIN ... ON` on the owner line;
- keep `WHEN ... THEN UPDATE SET` and similar actions on the owner line.

## 10. Lists and authored groups

Applies to:

- `SELECT`;
- `RETURNING`;
- `SET`;
- `VALUES`;
- `ORDER BY`;
- `GROUP BY`;
- function arguments;
- CTE lists;
- DDL action lists.

Rules:

- preserve existing non-empty line groups while they fit within the hard limit;
- blank lines are hard group boundaries;
- comments are hard group boundaries;
- never merge across a blank line or comment;
- do not split an authored group only because it exceeds the soft limit;
- split at safe item boundaries if it exceeds the hard limit;
- if a long one-line list must expand and has no authored groups, emit one item per line;
- never infer business-semantic groups.

## 11. Comments

- Preserve text and comment syntax.
- A standalone comment belongs to the following syntax element.
- An inline comment belongs to the current expression.
- Do not move comments across expressions or groups.
- Do not convert `--` to `/* ... */` or vice versa.
- A comment may exceed the hard limit when moving it would change attachment.

## 12. Boolean expressions

```sql
WHERE
    item.deleted_at IS NULL
    AND (
        item.title_rus IS NOT NULL
        OR item.title_orig IS NOT NULL
    )
```

Short cohesive child groups may remain inline:

```sql
AND (
    (lock.source = 'kp' AND lock.entity_id = link.kp_id)
    OR (lock.source = 'imdb' AND lock.entity_id = link.imdb_id)
)
```

Expand a child group further only when it is independently long or complex.

## 13. Statement-specific requirements

### 13.1 `JOIN ... ON`

Simple:

```sql
JOIN public.items item ON item.id = source.item_id
```

Complex:

```sql
LEFT JOIN source_links link ON
    link.item_id = item.id
    AND link.status = 'approved'
```

Do not create an `ON`-only indentation tier.

### 13.2 `CASE`

`CASE` is an expression. Short results remain after `THEN`:

```sql
CASE
    WHEN item.id IS NULL THEN 0
    WHEN item.deleted_at IS NOT NULL THEN -1
    ELSE score
END
```

A complete short `CASE` may remain inline.

### 13.3 CTEs

Each CTE starts on a new line after the previous `),`.

```sql
WITH first_cte AS (
    SELECT ...
),
second_cte AS (
    SELECT ...
)
SELECT ...;
```

### 13.4 Set operations

Use blank lines around `UNION`, `UNION ALL`, `INTERSECT`, and `EXCEPT`.

Preserve parentheses and operation precedence.

### 13.5 `VALUES`

- Short rows may remain inline.
- Complex rows expand independently.
- Adjacent rows may use different layouts.

### 13.6 `ON CONFLICT`

Keep the conflict-target predicate attached to `ON CONFLICT`.

Separate:

- conflict target;
- `DO UPDATE`;
- `SET`;
- action `WHERE`.

### 13.7 `UPDATE`

If `SET` expands, emit one assignment per line.

### 13.8 `DELETE`

Use the general compact-or-expanded rules. No extra layout model is required.

### 13.9 `MERGE`

- Separate `WHEN` branches with blank lines.
- Keep the action on the `WHEN ... THEN` line.
- Expand only the action arguments.
- Keep simple `USING ... ON` inline; expand after `ON` when complex.

### 13.10 DDL

- Keep simple `CREATE INDEX` statements compact.
- In `CREATE TABLE`, use one column or table constraint per line.
- Separate columns from table constraints with a blank line.
- In `ALTER TABLE`, group different action categories with blank lines.

### 13.11 Functions and procedures

Separate:

- signature;
- `RETURNS`;
- routine attributes;
- body introducer.

For `BEGIN ATOMIC`, indent body statements one level.

For dollar-quoted PL/pgSQL, `DECLARE`, `BEGIN`, `EXCEPTION`, and `END` are at the body root; `AS $$` does not add an indentation level.

### 13.12 PL/pgSQL branches

`IF`, `ELSIF`, `ELSE`, and exception handlers contain statements. Statements begin on the next line.

```sql
IF condition THEN
    statement;
END IF;
```

```sql
EXCEPTION
    WHEN foreign_key_violation THEN
        RAISE NOTICE 'missing';

    WHEN unique_violation THEN
        RAISE NOTICE 'duplicate';
END;
```

- Align `EXCEPTION` with `BEGIN`.
- Indent each `WHEN` one level.
- Indent handler statements one additional level.
- Separate multiple handlers with a blank line.

### 13.13 Embedded and templated SQL

When protected non-SQL regions are provided:

- preserve them byte-for-byte;
- format only SQL regions;
- keep SQL-relative indentation independent from host-language indentation;
- allow leading and trailing newlines in multiline SQL literals;
- do not move or rewrite template delimiters or control structures.

## 14. `check` behavior

`check` reports non-compliance with mandatory rules.

It must not report:

- a line only for crossing the soft limit;
- an existing valid authored grouping;
- `<>` under the default `not_equal_policy = preserve`;
- optional alignment differences.

Suggested diagnostic classes:

```text
casing.keyword
casing.builtin
casing.function
casing.type

spacing.binary_operator
spacing.comma
spacing.cast
spacing.function_call
spacing.sql_parenthesis
spacing.trailing_whitespace

indent.nesting
layout.hard_line_width
layout.boolean_group
layout.join_on
layout.authored_group
layout.comment_attachment
layout.case
layout.cte
layout.set_operation
layout.values
layout.on_conflict
layout.update_set
layout.merge
layout.ddl
layout.routine
layout.exception_handler

syntax.parse_failure
syntax.unsupported
format.statement_skipped
```

Each diagnostic should contain:

```text
rule_id
severity
message
source_range
fix_available
```

Style diagnostics are errors by default. Syntax diagnostics are separate from style compliance.

## 15. `fmt` behavior

`fmt` fixes deterministic, safe formatting violations.

It may fix:

- casing;
- mandatory spacing;
- indentation;
- safe line breaks;
- clause layout;
- comment-preserving blank lines;
- configured semicolon and `<>` normalization.

It must not guess when:

- comment attachment is ambiguous;
- syntax is unsupported;
- parsing failed;
- safe grouping cannot be determined.

If parsing failed before trustworthy statement boundaries exist, return the
complete original source and a diagnostic. If one parser-proven statement
cannot pass formatter ownership or safety gates, preserve that statement and
apply the statement-granular default/strict policy from section 5.3.

## 16. Acceptance criteria

Required tests:

1. Golden bad/good fixtures for every mandatory rule.
2. Idempotence tests.
3. Parse-format-parse tests when parser support exists.
4. Comment and blank-line preservation tests.
5. Authored-group preservation tests.
6. Soft/hard boundary tests with configurable widths.
7. Already-compliant input remains unchanged.
8. Malformed SQL returns unchanged source; unsupported or unformattable
   parser-proven statements remain byte-identical and produce diagnostics.
9. PL/pgSQL `CASE`, `IF`, and multiple `EXCEPTION` handler tests.
10. CTE, set operation, `ON CONFLICT`, `MERGE`, DDL, function, and procedure fixtures.
11. Embedded/template protected-range tests.
12. Property or fuzz tests for panic-free behavior and idempotence.

## 17. Definition of done

The `fmt/check` core is complete when:

- all acceptance tests pass;
- formatting is idempotent;
- failed formatting never returns a partially formatted statement or a
  partially written file;
- default configuration matches this specification;
- `check(format(source))` reports no style violations for successfully formatted input;
- the formatter does not alter SQL semantics in the maintained regression corpus.
