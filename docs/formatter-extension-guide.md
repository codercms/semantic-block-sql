# Extending PostgreSQL syntax support

Status: **implemented contributor workflow**

This guide describes the required code path for adding PostgreSQL syntax to the
formatter. It supplements [`formatter-architecture.md`](formatter-architecture.md)
and does not define formatting policy; the core specification remains
[`semantic-block-sql-fmt-check-core-spec.md`](semantic-block-sql-fmt-check-core-spec.md).

## Core rule

PostgreSQL parser acceptance is necessary but not sufficient. A syntax shape is
supported only when all three layers agree:

1. AST validation accepts and describes the shape.
2. Token ownership binds and verifies that description.
3. A planner has fixtures for its formatting behavior.

If AST capability is missing, return the original statement with
`syntax.unsupported`. If an accepted statement later fails ownership or a
safety gate, preserve that complete statement with `format.statement_skipped`
under the configured default/strict policy. Never let a known keyword fall
through generic whitespace normalization.

## Existing extension points

| Concern | Main type/function | Location |
| --- | --- | --- |
| Parser-version gate | `REVIEWED_POSTGRESQL_VERSION` | `validation.rs` |
| Statement dispatch | `StatementSpec` | `ownership.rs` |
| Family capabilities | `InsertSpec`, `MergeSpec`, etc. | `ownership.rs` |
| AST classification | `validate_statement` | `validation.rs` |
| Family validation | `validate_insert`, `validate_merge`, etc. | `validation.rs` |
| Source/token binding | `bind_token_statements` | `ownership.rs` |
| Statement binding | `bind_*` | `layout_ir/statement.rs` |
| Query binding | `bind_queries`, `bind_predicates` | `layout_ir/query.rs` |
| Layout dispatch | `StatementLayout` | `layout_ir.rs` |
| Statement planning | `plan_*_statements` | `semantic_block/statements.rs` |
| Owned expressions | `owned_expression_ranges` | `semantic_block/expressions.rs` |
| Shared layout decision | `LayoutGroup::decide` | `semantic_block/groups.rs` |
| Shared lists | `plan_keyword_list` and related helpers | `semantic_block/lists.rs` |
| Token rendering | `render_token`, `needs_space` | `semantic_block/render.rs` |
| Final safety | `format_sql`, `validate_equivalent` | `mod.rs`, `validation/equivalence.rs` |

## Adding a variant to an existing statement

Example: supporting another INSERT source or clause.

### 1. Characterize PostgreSQL's AST

Add a focused fixture or temporary unit test and inspect the pinned `pg_query`
protobuf fields. Record:

- the owning top-level node;
- nested nodes and enum values;
- cardinalities;
- whether source locations exist;
- adjacent syntax that must remain unsupported.

Do not infer ownership only from token order.

### 2. Extend the family capability record

Update the corresponding `*Spec` type in `ownership.rs`. Prefer closed enums and
counts over unrelated booleans:

```rust
pub(super) enum InsertSourceSpec {
    DefaultValues,
    Values { rows: usize },
    Query { set_operations: usize },
}
```

A field belongs in the capability record when the token binder can verify it.
Avoid documentary fields that are never consumed.

### 3. Extend AST validation

Update the family validator in `validation.rs` and return the new capability.
Validate nested expressions and reject nearby unowned forms explicitly.

The validator must not expose the protobuf tree to the renderer. It should
return only the formatter-relevant contract.

### 4. Extend token binding

Update the relevant binder in `layout_ir/statement.rs` or
`layout_ir/query.rs`.

The binder must:

- search only inside its AST-owned half-open token range;
- respect `base_depth`;
- locate presentation boundaries;
- compare clause presence, modes, and cardinalities with the `*Spec`;
- return `FormatDiagnostic::Ownership` on disagreement.

Do not add a document-wide keyword scan.

### 5. Reuse or extend layout IR

Use shared records where possible:

- `TokenSpan`;
- `QueryBlock` and `QueryClauses`;
- `PredicateBlock`;
- `WithBlock`;
- delimited list ownership;
- assignment and RETURNING list planning.

Add a statement-specific record only for genuinely statement-specific grammar.

### 6. Add planner behavior

Use typed ranges from `semantic_block/expressions.rs`, shared functions in
`semantic_block/lists.rs`, the common Boolean planner, and the token renderer.
Keep statement-specific decisions in `semantic_block/statements.rs`.

When a newly supported construct owns an expression, derive that expression
from the construct's existing typed layout record. Add an exhaustive
`ExpressionOwnerKind` case only when its root indentation or attachment policy
differs from existing owners. Do not discover expressions through a global
`AND`/`OR` scan. Parenthesized lists must classify independently complex
Boolean items during the first pass so list expansion remains idempotent.

The planner must not duplicate AST validation. It receives already-verified
ownership. Reuse the pass-wide `PlanningContext`; do not add another collection
of token/depth/list/options parameters or suppress `clippy::too_many_arguments`.
Use `LayoutGroup::decide` for compact-versus-expanded policy. A new owner may
supply different safe breakpoints or an authored-group requirement, but it must
not implement another width/comment/complexity decision tree.

For set operations, bind the complete expression owner and all branches inside
that owner. Never represent an operation as an operator plus a forward search
for the next query keyword, and never rescan CTEs or statements for `UNION` in
the planner.

### 7. Add evidence

Fixtures must include, where applicable:

- compact input and output;
- width-driven expansion;
- comments at every ownership boundary;
- authored list groups;
- semantic equivalence;
- idempotence;
- clean `check` output after formatting;
- adjacent unsupported shapes remaining byte-identical.

## Adding a new statement family

A new top-level family requires compiler-visible changes:

1. Add a capability struct and `StatementSpec` variant.
2. Add `expected_token`, `family_name`, and `has_with` match arms.
3. Add a `validate_statement` branch and family validator.
4. Add a `StatementLayout` variant.
5. Add a binder in `layout_ir/statement.rs`.
6. Add an iterator/accessor only if planning needs a family collection.
7. Add a planner and call it from `semantic_block::format`.
8. Add diagnostics and fixtures.

Do not introduce a runtime registry to avoid these exhaustive matches. Compiler
errors are intentional: they reveal every top-level integration point.

## PostgreSQL parser upgrades

A `pg_query` upgrade changes the embedded PostgreSQL grammar version. The
formatter fails closed until `REVIEWED_POSTGRESQL_VERSION` is updated.

Upgrade procedure:

1. Inspect protobuf changes for all currently supported nodes.
2. Review newly added enum variants and fields.
3. Run the complete characterization and golden suite.
4. Add unsupported fixtures for newly parsed but unowned syntax.
5. Update capability records and binders only for intentionally supported
   additions.
6. Update the reviewed version constant in the same commit.

A parser upgrade must never silently broaden formatter support.

## Range and index conventions

- Source ranges are UTF-8 byte offsets.
- `TokenRange` is always half-open: `[start, end)`.
- A statement's terminal semicolon is stored separately.
- Clause indices point into the one immutable token slice for that formatting
  pass.
- Any new range type must document whether its end is inclusive or exclusive;
  mixed semantics are forbidden.

## Required validation gate

```text
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo doc --locked --no-deps
git diff --check
```

Before committing, confirm:

- validation and binding capabilities agree;
- unsupported neighboring syntax is unchanged;
- comments and protected tokens are byte-identical;
- no document-wide statement discovery was introduced;
- no existing statement planner was rewritten merely to add another family;
- dead capability fields and duplicate helpers were removed.


## Extension paths after the expanded coverage tranche

Three extension paths now exist and must remain distinct:

1. **Layout-bearing statement or query syntax** extends `StatementSpec` or a
   query/relation capability record, then updates binding and planning.
2. **Simple migration utilities** may extend `UtilityStatementKind` only when
   the exact protobuf node and every accepted option shape are validated; this
   path must never become a generic utility fallback.
3. **PL/pgSQL syntax** extends the `parse_plpgsql` node allowlist and the explicit
   frame/line renderer in `procedural.rs`, with fixtures for nested indentation,
   comments, protected literals, normalized parser equivalence, and idempotence.

When adding CTEs or subqueries, tests must prove that the binder identifies the
owned root statement rather than the first nested SELECT. When adding relation
or table variants, include an adjacent parsed-but-unowned form. When adding a
procedural node, include both nested control flow and a still-unsupported sibling
node so the allowlist cannot broaden accidentally.

## Unsupported-policy requirements

Every new syntax family must distinguish valid-but-unowned syntax from malformed input. Valid unsupported units must be preserved exactly and reported as warnings under the default policy; tests must also prove strict-policy error elevation and no writes. Add mixed-document fixtures showing that unsupported units do not suppress supported siblings. Utilities may share a renderer only through a closed AST-validated capability enum, with explicit ownership for nested SQL and protected payloads.

## Extending PL/pgSQL after the IR rewrite

A new procedural feature must first be mapped from its exact `parse_plpgsql` node name into a typed parser category, then classified into a source-span `BodyNodeKind`, and finally rendered by the procedural layout/leaf layers. Include compact and multiline fixtures, nested control flow, comment/protected-literal cases, parser-alignment tests, equivalence, idempotence, and an unsupported sibling. Do not reintroduce line-prefix syntax discovery.
