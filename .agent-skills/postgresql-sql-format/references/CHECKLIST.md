# Formatting completion checklist

Use this after formatting a non-trivial statement.

## Semantic preservation

- No expression, predicate, join, CTE, assignment, result column, or set-operation branch was reordered.
- No cast, alias, literal, clause, or parenthesis was added or removed.
- No join type or query structure changed.
- No operator changed except PostgreSQL `<>` to preferred `!=`.
- Comments still describe the same syntax node.
- Quoted identifiers, strings, and dollar-quoted content are unchanged.

## Structure

- Every genuinely nested statement or argument block is indented by four spaces.
- Structural connectors such as `ON` and `THEN` do not create redundant keyword-only indentation levels.
- Top-level statement clauses are easy to locate.
- Long or complex argument lists are expanded at natural boundaries.
- Related short arguments remain grouped where this improves scanning.
- Mixed `AND`/`OR` logic has explicit and visible grouping.
- `AND` and `OR` begin continuation lines.
- Blank lines separate logical blocks rather than arbitrary clauses.

## Lexical style

- SQL keywords and `NULL`/`TRUE`/`FALSE` are uppercase.
- Function names and type names are lowercase.
- PostgreSQL not-equal comparisons use `!=`.
- Binary operators have surrounding spaces.
- Commas are trailing and followed by one space in inline lists.
- Casts use `value::type` without spaces.
- No trailing whitespace remains.
- The semicolon is attached to the final clause.

## Statement-specific checks

- `CASE` keeps simple `WHEN ... THEN ...` and `ELSE ...` branches compact.
- `ON CONFLICT` clearly distinguishes its target predicate from the update-action predicate.
- `MERGE` branches are separated; each action introducer stays on its `WHEN ... THEN` line and only its arguments are indented.
- Set operators are visually separated without changing precedence.
- Simple indexes remain compact; complex indexes expand only as needed.
- SQL inside PL/pgSQL follows the same SQL rules.

## Output

- Return only formatted SQL unless the user requested commentary, review findings, or a diff.
- When semantic concerns are noticed, do not silently fix them during formatting.
