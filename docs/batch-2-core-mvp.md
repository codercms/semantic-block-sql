# Batch 2 formatter-core MVP

Status: **complete**

Date: **2026-07-25**

## Outcome

Batch 2 is the formatter-core MVP. The pure `format_sql` facade now has enough
layout behavior to prove Semantic Block SQL's distinguishing rules before
adding broad statement coverage or filesystem-facing CLI code.

Implemented and fixture-backed:

- compact and expanded `SELECT` result lists;
- compact and expanded function-argument lists;
- source-authored list line groups;
- blank-line and comment hard boundaries;
- deterministic packing of one-line input;
- configurable indentation and soft/hard widths;
- cohesive authored groups allowed beyond soft width;
- over-hard group splitting at comma boundaries;
- warnings for indivisible over-hard tokens;
- compact and expanded JOIN predicates, including width-driven expansion;
- mixed `AND`/`OR` precedence layout;
- compact and expanded `CASE` with compact branches;
- multiple CTEs;
- recursive CTE anchor and recursive branch separation;
- exact comments, literals, quoted identifiers, and dollar contents;
- PostgreSQL parse-before/after, canonical AST equality, and idempotence.

## Layout architecture

No new crate was added. The existing pinned `pg_query 6.1.1` scanner supplies:

- token kinds and exact source byte ranges;
- gaps between tokens, from which authored line and blank-line hints are
  derived;
- comments as ordinary ordered tokens;
- the PostgreSQL parse tree used by the semantic-equivalence gate.

The project-specific layout layer builds a token-indexed break plan. Separate
planning passes cover lists, query clauses, boolean expressions, `CASE`, and
CTEs. The writer then changes only whitespace and allowed lexical casing while
retaining token order.

This avoids both a handwritten SQL parser and a second CST dependency.

## Width behavior

`FormatOptions` defaults remain:

```text
indent_width = 4
soft_line_width = 120
hard_line_width = 160
preserve_list_groups = true
preserve_blank_lines = true
```

One-line simple lists are packed greedily to soft width. Authored groups are
not split merely for crossing soft width; a group is split only at a comma
boundary when it crosses hard width.

Every formatted line is checked after layout. A breakable over-hard line
returns `FormatDiagnostic::HardLineExceeded`. An indivisible source token may
exceed hard width and produces:

```rust
FormatWarning::IndivisibleTokenExceedsHardWidth { line, width }
```

## Safety review

- Layout never reorders scanner tokens.
- PostgreSQL parses both original and formatted SQL.
- Canonical parse trees must remain equal after removing source locations only.
- Protected token text and order must remain byte-identical.
- SQL line comments always retain a physical newline.
- Quoted function and type identifiers are excluded from case normalization.
- Formatting the result again must return identical bytes.
- No filesystem mutation exists in the formatter crate.

## Deliberate limits

Batch 2 does not claim general statement coverage. `INSERT`, `VALUES`,
`ON CONFLICT`, `UPDATE`, `DELETE`, `MERGE`, general set operations, windows,
DDL, and PL/pgSQL remain Batch 3 and require their own fixtures.

The recursive CTE fixture covers `UNION ALL` only as the required recursive CTE
shape; it is not a claim of general set-operation layout.

## Verification

```text
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
git diff --check
```

Batch 1 and Batch 2 tests jointly cover the public formatter facade. Each
golden fixture also runs structural equivalence and a second formatting pass.
