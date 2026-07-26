# Formatter architecture

Status: **implemented architecture and extension guide**

This document describes how the formatter currently works at code level and
how new PostgreSQL syntax should be added without duplicating a parser or
turning the layout engine into a collection of unrelated keyword scans.

The authoritative formatting behavior remains
[`semantic-block-sql-fmt-check-core-spec.md`](semantic-block-sql-fmt-check-core-spec.md).
This document describes implementation structure, not additional style rules.

## Design goals

The formatter is designed around five constraints:

1. PostgreSQL syntax ownership comes from PostgreSQL's parser, not handwritten
   SQL parsing.
2. Exact authored tokens, comments, literals, and line boundaries remain
   available to the renderer.
3. Unsupported or newly introduced PostgreSQL syntax is preserved unchanged.
4. A statement family can be added without rewriting existing planners.
5. `fmt` and `check` share one canonical formatting result and safety gates.

## Module map

```text
src/formatter/
├── mod.rs             public facade and safety pipeline
├── validation.rs      PostgreSQL AST support classification
├── ownership.rs       AST-validated statement ownership IR
├── tokens.rs          exact scanner tokens and authored gaps
├── semantic_block.rs  source-preserving layout planning and writing
└── diagnostics.rs     rule-level diagnostics and source ranges
```

Filesystem discovery, SQL directives, Go extraction, diff generation, and
atomic rewriting remain outside the formatter core.

## End-to-end flow

```mermaid
flowchart TD
    Source[Source SQL] --> Parse[pg_query parse]
    Parse --> Validate[AST support validation]
    Validate --> Ownership[SupportedDocument ownership IR]
    Source --> Scan[pg_query scan]
    Ownership --> Bind[Bind byte spans to token spans]
    Scan --> Bind
    Bind --> Analyze[Shared layout analysis]
    Analyze --> Plan[LayoutPlan]
    Plan --> Write[Token-preserving writer]
    Write --> Reparse[Parse and classify output]
    Reparse --> Equivalent[Canonical AST equality]
    Equivalent --> Protected[Protected-token equality]
    Protected --> Idempotent[Second formatting pass]
    Idempotent --> Result[Output and diagnostics]
```

`format_sql` in `mod.rs` owns this sequence. It parses and validates the input,
passes the resulting ownership model into the layout engine, validates the
formatted output, and requires a byte-identical second pass.

`format_sql_result` wraps failures in a fail-safe value result: the original
source is returned unchanged with a diagnostic.

## Ownership IR

`ownership.rs` is the boundary between PostgreSQL's AST and token layout.

### `StatementKind`

```rust
pub(super) enum StatementKind {
    Select,
    Insert,
    Update,
    Delete,
}
```

This closed Rust sum type is the central top-level dispatcher. A new statement
family is added as a new variant only when its AST validation and layout planner
are introduced together.

Unknown PostgreSQL node variants do not enter the planner. They produce
`syntax.unsupported` and preserve the original statement.

### `SourceStatement`

```rust
pub(super) struct SourceStatement {
    pub kind: StatementKind,
    pub start: usize,
    pub end: usize,
}
```

A source statement owns one UTF-8 byte span from PostgreSQL's `RawStmt`
locations. The terminal semicolon is included when present.

### `SupportedDocument`

```rust
pub(super) struct SupportedDocument {
    statements: Vec<SourceStatement>,
}
```

This is produced by `validation::parse_supported_postgresql`. It proves that
every top-level statement and every checked nested construct belongs to the
fixture-backed support boundary.

It intentionally contains only formatter-relevant ownership metadata rather
than exposing the full protobuf tree to every layout function.

### `TokenStatement`

```rust
pub(super) struct TokenStatement {
    pub kind: StatementKind,
    pub start: usize,
    pub end: usize,
    pub base_depth: usize,
}
```

`bind_token_statements` maps AST-owned byte spans to scanner-token spans.
Statement planners receive only these owned spans. They no longer discover DML
statements by scanning the complete document for words such as `INSERT`,
`UPDATE`, or `DELETE`.

If an AST-supported statement cannot be bound to the expected token shape, the
formatter returns `FormatDiagnostic::Ownership`. This is an internal safety
failure, not a silent fallback.

## AST support classification

`validation.rs` performs two related checks.

### Top-level statement classification

`validate_statement` matches PostgreSQL protobuf sum types and returns a
`StatementKind` for the exact supported family:

```rust
match node {
    NodeEnum::SelectStmt(select) => ...,
    NodeEnum::InsertStmt(insert) => ...,
    NodeEnum::UpdateStmt(update) => ...,
    NodeEnum::DeleteStmt(delete) => ...,
    _ => Err("unimplemented PostgreSQL statement family"),
}
```

Family-specific validators check the currently owned AST shape. For example,
UPDATE currently rejects multi-column assignment targets and complex FROM
sources because no planner owns them yet.

### Nested-node validation

After top-level classification, nested PostgreSQL nodes are traversed to reject
unowned constructs such as unsupported subqueries, window forms, or lateral
sources.

This makes support explicit. Successful parsing alone never implies formatter
support.

## Token and source model

`tokens.rs` calls `pg_query::scan` and stores:

```rust
pub(super) struct SqlToken<'a> {
    pub kind: Token,
    pub keyword_kind: KeywordKind,
    pub text: &'a str,
    pub start: usize,
    pub end: usize,
    pub line_breaks_before: usize,
}
```

The AST supplies semantic ownership; scanner tokens supply exact source text.
The formatter does not deparse the protobuf AST because deparsing would discard
comments, authored groups, and original token spelling.

## Layout planning

`semantic_block.rs` builds a token-indexed `LayoutPlan`:

```rust
struct LayoutPlan {
    before: HashMap<usize, Break>,
    token_indents: Vec<Option<usize>>,
}
```

Planning functions request line breaks and indentation. The writer later emits
tokens in their original order.

Current shared analyses include:

- parenthesis pairs and syntactic depth;
- source-aware comma-separated list items;
- compact versus expanded CASE ranges;
- boolean predicate ranges;
- CTE ownership;
- terminal semicolon policy;
- soft and hard width decisions.

DML planners are now dispatched from `TokenStatement` ownership. Their internal
clause structures—`InsertBlock`, `UpdateBlock`, `DeleteBlock`, and
`OnConflictBlock`—are token-range layout IR for the currently supported shape.

These structures are not PostgreSQL parsers. They locate clause boundaries only
inside an AST-validated statement span whose legal shape is already known.

## Writer

The writer changes only permitted presentation details:

- whitespace;
- indentation;
- line breaks;
- required keyword/function/type casing;
- configured `<>` to `!=` normalization;
- configured terminal-semicolon policy.

It never reorders tokens or synthesizes arbitrary SQL structure.

## Safety gates

A formatting result is accepted only when all gates pass:

1. input parses as PostgreSQL;
2. input AST shape is supported;
3. every supported statement binds to its token span;
4. output parses and passes the same support classifier;
5. canonical ASTs are equal after source locations are removed;
6. literals, quoted identifiers, comments, and other protected tokens are
   byte-identical and ordered identically;
7. a second formatting pass is byte-identical;
8. every breakable line respects the hard-width policy.

No file is partially rewritten. The CLI plans every project rewrite before the
first atomic replacement.

## Adding PostgreSQL syntax

Use this sequence for each syntax extension.

### 1. Characterize the AST

Add a focused test or temporary inspection against the pinned `pg_query`
protobuf representation. Identify the owning top-level and nested node fields.
Do not infer grammar ownership from keywords alone.

### 2. Extend the closed ownership model

For a new statement family, add a `StatementKind` variant. For a new variant of
an existing family, extend its family-specific validated shape instead.

Do not create a dynamic registry merely to avoid a Rust `match`; exhaustive
matches are useful because compiler errors identify every dispatcher that must
handle a new family.

### 3. Extend validation

Update `validate_statement` and the relevant family validator. Keep adjacent
unimplemented shapes unsupported.

When PostgreSQL adds a protobuf node or field that the formatter does not know,
the default outcome must remain unchanged source plus `syntax.unsupported`.

### 4. Add or extend layout IR

Prefer reusable ownership concepts:

- statement span;
- clause span;
- delimited list;
- expression list;
- assignment list;
- predicate;
- nested statement;
- branch list.

Add a statement-specific structure only where PostgreSQL grammar has genuinely
statement-specific ownership, such as ON CONFLICT or MERGE branches.

### 5. Add fixtures before claiming support

Each supported shape requires:

- compact and expanded golden output where applicable;
- comments at ownership boundaries;
- semantic-equivalence validation;
- idempotence;
- clean `check` output after formatting;
- neighboring unsupported variants that remain byte-identical.

### 6. Run the complete gate

```text
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo doc --locked --no-deps
git diff --check
```

## Forward compatibility policy

A PostgreSQL upgrade can produce four outcomes:

| Parser result | Ownership result | Formatter behavior |
| --- | --- | --- |
| parser rejects syntax | none | unchanged source plus parse diagnostic |
| parser accepts unknown statement family | unsupported | unchanged statement plus `syntax.unsupported` |
| known family contains unknown/unowned shape | unsupported | unchanged statement plus `syntax.unsupported` |
| known and fixture-backed shape | supported | structural formatting and full safety gates |

This policy lets the parser be upgraded independently without allowing new
syntax to fall accidentally into generic whitespace normalization.

## Current migration boundary

The top-level DML path now uses `SupportedDocument` and `TokenStatement`.
SELECT, CTE, CASE, boolean, and generic parenthesized-list discovery still use
whole-document token passes after AST validation.

Future architecture batches should migrate these analyses to owned statement or
expression spans when doing so reduces ambiguity. They should not perform a
large rewrite merely for structural uniformity; each migration must accompany
new syntax coverage or remove concrete duplicated ownership logic.

The next intended extensions are:

1. shared nested-query ownership;
2. `INSERT ... SELECT` and `OVERRIDING`;
3. DML `WITH` using the same CTE ownership model;
4. general query/set-operation ownership;
5. MERGE branch ownership;
6. DDL and routine ownership;
7. PL/pgSQL body ownership.
