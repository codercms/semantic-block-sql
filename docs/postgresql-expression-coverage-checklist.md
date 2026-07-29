# PostgreSQL expression coverage — durable checklist

Status: **Batch E1 complete; Batch E2 pending**

This workstream completes the remaining Batch 3 scalar-expression tranche after
Go host-language integration hardening. Update this file before every checkpoint
commit. A checked item requires focused tests and the batch self-review defined
in `AGENTS.md`.

## Decisions

- The authoritative behavior remains `docs/semantic-block-sql-fmt-check-core-spec.md`.
- Scope is bounded to PostgreSQL casts, array constructors/types/subscripts/slices,
  and PostgreSQL JSON operators already represented as ordinary scalar AST nodes.
- `ARRAY[...]`, array type suffixes such as `text[]`, subscripts such as `tags[1]`,
  and slices such as `tags[1:3]` are tight punctuation: no space before `[` and no
  spaces around a slice colon.
- `ARRAY (SELECT ...)` keeps the existing grammar-parenthesis spacing; this batch
  does not change nested-query ownership.
- JSON operators are binary operators and therefore keep one space on both sides.
- SQL/JSON constructor/query families (`JSON_OBJECT`, `JSON_QUERY`, `JSON_TABLE`,
  and related protobuf nodes) remain unsupported until separately fixture-backed.
- No new dependency is permitted for this tranche.

## AST characterization

The pinned PostgreSQL 17 grammar (`pg_query 6.1.1`, protobuf version `170004`)
represents the reviewed shapes as:

- `TypeCast` + `TypeName` for both `value::type` and `CAST(value AS type)`;
- `AArrayExpr` for `ARRAY[...]` constructors;
- `AIndirection` + `AIndices` for subscripts and slices;
- `TypeName.array_bounds` for array type suffixes;
- `AExpr` with `AexprOp` and operator-name strings for PostgreSQL JSON operators.

These are scalar-expression shapes inside already-owned statements. They do not
require a new statement or clause ownership variant.

## Batch E0 — Plan and baseline

- [x] Continue from pushed PR 9 head `965404db4e4e11514c3b7719af8671bcd830b86f`.
- [x] Read repository instructions, core spec, architecture, extension guide, and
  canonical SQL skill.
- [x] Characterize the pinned protobuf AST for casts, arrays, slices, and JSON
  operators.
- [x] Run the complete 138-test baseline successfully.
- [x] Record bounded scope and adjacent unsupported SQL/JSON families.
- [x] Create branch `agent/postgresql-expression-coverage`.
- [x] Commit Batch E0 before behavior changes.

## Batch E1 — Cast and array rendering

- [x] Add the smallest failing golden fixture for square-bracket spacing.
- [x] Render `ARRAY[...]`, `expr[...]`, chained subscripts, and array type suffixes
  without a space before `[`.
- [x] Render slice bounds without spaces around `:` including omitted bounds.
- [x] Cover `::`, `CAST(...)`, type modifiers, schema-qualified types, and array
  types without changing literal or identifier bytes.
- [x] Cover one-dimensional and multidimensional constructors, subscripts, and
  slices.
- [x] Verify comments, authored list groups, semantic equivalence, idempotence,
  clean `check`, and hard-width behavior.
- [x] Run focused tests and Batch E1 self-review.
- [x] Commit Batch E1.

Batch E1 evidence:

- `tests/fixtures/batch7/casts-arrays.*.sql` covers `::`, `CAST`, qualified
  custom types, one- and multidimensional array types and constructors, chained
  subscripts, every omitted-bound slice form, and DDL array suffixes.
- `tests/batch7_expressions.rs` requires exact golden output, semantic
  equivalence, idempotence, clean `check`, comment preservation, and bounded
  narrow-width output.
- Square brackets are tight punctuation. Slice colons are recognized only at the
  active square-bracket level, so a SQL/JSON constructor colon nested inside an
  array element keeps ordinary grammar spacing.
- The cast-type classifier follows only an explicit `::`, a `CAST(... AS ...)`
  marker, or a qualified-name dot chain. An implicit alias after a cast is not
  lowercased as though it were another type component.

## Batch E2 — JSON operators and fail-closed SQL/JSON boundary

- [ ] Add golden fixtures for `->`, `->>`, `#>`, `#>>`, `@>`, `<@`, `?`, `?|`,
  `?&`, `#-`, and `||` in realistic expressions.
- [ ] Verify JSON operators use binary-operator spacing and preserve JSON/path
  literal bytes exactly.
- [ ] Cover JSON expressions in SELECT, predicates, assignments, VALUES, and
  RETURNING through existing statement ownership.
- [ ] Explicitly reject unreviewed SQL/JSON protobuf families with
  `syntax.unsupported` and unchanged source.
- [ ] Add neighboring unsupported fixtures for SQL/JSON constructors/query forms.
- [ ] Run focused tests and Batch E2 self-review.
- [ ] Commit Batch E2.

## Batch E3 — Reconciliation and final gate

- [ ] Mark casts, arrays, and JSON expressions complete in
  `docs/implementation-checklist.md`.
- [ ] Document the expression punctuation and support boundary in formatter docs.
- [ ] Run `cargo fmt --all -- --check`.
- [ ] Run `cargo clippy --locked --all-targets -- -D warnings`.
- [ ] Run `cargo test --locked --all-targets`.
- [ ] Run `cargo doc --locked --no-deps`.
- [ ] Run `git diff --check`.
- [ ] Perform final semantic, architecture, idempotence, comment, diagnostics,
  dependency, and dead-code self-review.
- [ ] Commit the final checkpoint.
- [ ] Push the branch or create and verify a portable git bundle.
