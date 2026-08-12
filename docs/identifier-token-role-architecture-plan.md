# Parser-owned token roles and identifier preservation

Status: implemented; local quality gate complete

## Problem

PostgreSQL scanner categories are lexical, not semantic. A token such as `no`
is grammar in `FOR NO KEY UPDATE`, but an identifier in
`FROM numbered_offers no`. The formatter currently lets the shared renderer
uppercase selected scanner token kinds without first proving that the token is
grammar. That can silently change the authored spelling of aliases and other
identifiers.

This batch removes scanner-only casing from parser-owned alias declarations.
Alias spelling is classified by the validated PostgreSQL AST and bound
ownership model before the shared renderer sees it. Ambiguous or contradictory
alias ownership fails closed for the complete statement.

## Architecture contract

- Build one token-role map after AST support validation and token ownership
  binding.
- Preserve every parser-owned alias and alias-column identifier byte-for-byte,
  including unquoted identifiers whose scanner token kind is also a PostgreSQL
  keyword.
- Uppercase only parser-owned grammar and the documented built-in whitelist.
- Lowercase only parser-owned function and type names covered by the core
  casing contract.
- Use the same token-role map for rendering, compact-width measurement,
  hard-width analysis, and casing diagnostics.
- Keep ordinary SQL, embedded Go SQL, and SQL statements inside routines on
  the canonical SQL renderer.
- Verify the bound identifier sequence and exact spelling after formatting in
  addition to AST equivalence, protected-token preservation, and idempotence.
- If an AST name cannot be bound exactly, or an ambiguous casing candidate has
  no role, preserve the complete statement and emit the existing ownership
  safety diagnostic.
- Do not add a keyword allowlist exception, global alias scan, permissive
  fallback, or dependency.

## Implementation checklist

### Regression characterization

- [x] Reproduce `FROM numbered_offers no` becoming
  `FROM numbered_offers NO`.
- [x] Add an initial exhaustive PostgreSQL 17 `ColId` relation-alias test.
- [x] Add an initial cross-owner alias test covering SELECT and supported DML.
- [x] Store the complete pinned PostgreSQL 17 keyword metadata used by tests:
  all keywords, scanner category, and bare-label status.
- [x] Generate test cases from the metadata rather than maintaining only the
  observed failure subset.
- [x] Cover all supported relation owners: comma-separated FROM, every JOIN
  header, parenthesized joins, derived queries, functions, `ROWS FROM`,
  `TABLESAMPLE`, UPDATE FROM, DELETE USING, and MERGE USING.
- [x] Cover target aliases for INSERT, UPDATE, DELETE, and MERGE.
- [x] Cover SELECT/RETURNING aliases with explicit and grammar-permitted
  omitted `AS`.
- [x] Cover CTE names and column aliases, named windows, view aliases, relation
  alias column lists, and function column-definition lists.
- [x] Add counter-fixtures proving grammatical `NO`, `FILTER`, `RANGE`,
  `VALUES`, `UPDATE`, and neighboring keywords still uppercase.

### Typed ownership

- [x] Add an internal identifier role that overrides scanner casing only after
  parser ownership is bound.
- [x] Retain parser-owned alias names and cardinalities in statement,
  query, relation-source, CTE, output-list, and window capabilities.
- [x] Replace unordered relation alias facts with ordered, owner-bounded typed
  relation ownership so aliases cannot escape their AST owner.
- [x] Bind AST names to exact token indices, using parser locations where
  available and owner-bounded structural binding for locationless `Alias`
  fields.
- [x] Reject missing, out-of-order, or contradictory bindings.
- [x] Require every parser-owned alias in the supported ownership model to
  resolve to an identifier role.

### Canonical rendering and safety

- [x] Make canonical SQL rendering consume token roles before global
  `is_keyword_like` decisions.
- [x] Make compact-width and hard-width calculations use the same role-aware
  spelling as final rendering.
- [x] Make diagnostics compare against the same role-aware expected spelling.
- [x] Keep grammar casing centralized; add no alias-specific keyword exception.
- [x] Add exact owned-identifier preservation to the post-format safety gate.
- [x] Prove deliberate validator/binder disagreement preserves the statement
  and reports an ownership safety diagnostic.
- [x] Verify Go extraction/rewrite and routine formatting continue to use the
  canonical formatter without local casing behavior.

### Documentation and completion gate

- [x] Record the parser-owned token-role decision in `formatter-design.md`.
- [x] Document token-role construction and failure boundaries in
  `formatter-architecture.md` and `formatter-extension-guide.md`.
- [x] Review `README.md`, `docs/user-guide.md`, and `docs/sql-coverage.md`;
  update only if their documented public behavior or coverage changes.
- [x] Run focused alias, query, DML, relation-source, routine, and Go tests.
- [x] Run `cargo fmt --all -- --check`.
- [x] Run `cargo clippy --locked --all-targets -- -D warnings`.
- [x] Run `cargo test --locked --all-targets`.
- [x] Run `cargo doc --locked --no-deps`.
- [x] Run `git diff --check`.
- [x] Complete the batch self-review for semantics, ownership boundaries,
  idempotence, comments/groups, diagnostics, atomicity, dependencies, and dead
  code.
- [x] Commit the coherent completed batch before unrelated work.

## Acceptance criteria

- Formatting `FROM numbered_offers no` preserves `no` exactly and produces no
  casing diagnostic after formatting.
- Every PostgreSQL 17 keyword accepted by the grammar in each tested alias or
  identifier position preserves its authored spelling.
- The same words remain uppercase in parser-owned grammatical positions.
- No supported statement can be emitted with a changed parser-owned alias or
  alias-column identifier.
- Unsupported or ownership-ambiguous statements remain byte-identical under
  the default policy.
- Public APIs, configuration, supported SQL coverage, and dependencies remain
  unchanged.
