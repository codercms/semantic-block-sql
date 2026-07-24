# Batch 1 backend spike

Status: **complete**

Date: **2026-07-25**

## Decision

The MVP does not patch or vendor `libpgfmt`. It uses `pg_query 6.1.1` as the
single PostgreSQL backend for:

- the PostgreSQL parser compiled from PostgreSQL 17.4 sources;
- the PostgreSQL scanner;
- exact token byte ranges, including comments and quoted contents;
- the protobuf parse tree used by the structural-equivalence gate.

Semantic Block layout remains project code because it is the product-specific
policy. The project does not implement a SQL parser or lexer.

The dependency is pinned exactly in `Cargo.toml` and `Cargo.lock`. Updating it
requires rerunning backend characterization, all golden fixtures, protected
token checks, structural equivalence, and idempotence.

## Toolchain

```text
rustc 1.88.0 (6b00bc388 2025-06-23)
commit-hash: 6b00bc3880198600130e1cf62b8f8a93494488cc
host: x86_64-unknown-linux-gnu
LLVM version: 20.1.5

cargo 1.88.0 (873a06493 2025-05-10)
```

`pg_query` builds a bundled C library and runs bindgen. Development and CI
therefore require a C toolchain and libclang. This is a build dependency, not a
runtime database dependency.

## Exact upstreams

| Project | Release source | Commit | License | Result |
| --- | --- | --- | --- | --- |
| `libpgfmt` | tag `v1.3.0` | `c4b8e7398f344eabaac21a38ea2c590212c53d93` | BSD-3-Clause | Characterized and rejected as the MVP base. |
| `tree-sitter-postgres` through `libpgfmt` | `19.0.0-beta.2` | `10163f867437b3527592624ef1ecb9fca0853971` | BSD-3-Clause | Useful CST, but its current grammar rejects `MERGE`. |
| `pg_query` crate `6.1.1` | crates.io release | `66eb7becea1a40e315fee3f90197a35a89d20c25` | MIT | Selected and pinned. |

The `pg_query 6.1.1` crate embeds `libpg_query` generated from PostgreSQL 17.4.
It accepts PostgreSQL `MERGE`; later PostgreSQL syntax requires a deliberate
backend upgrade and new fixtures.

## Unmodified upstream test result

The exact `libpgfmt v1.3.0` source was tested without changes:

```text
cargo test --locked --all-targets
initial build and tests: 21.366s

65 fixture tests passed
9 PL/pgSQL tests passed
23 smoke tests passed
9 parenthesis tests passed
aggregate fixture and pg_dump idempotency tests passed
```

## `libpgfmt` characterization

The spike exercised the public `format(sql, Style::Kickstarter)` API.

| Case | Observed result | Semantic Block impact |
| --- | --- | --- |
| Standalone top comment | Preserved | Acceptable. |
| Inline `SELECT` comment | Comment disappeared and the list contained a stray comma | Safety blocker. |
| `SELECT FROM;` | Accepted and rewritten to `SELECT *;` | Semantic safety blocker. |
| `SELECT 800.00` | Accepted | Grammar recovery works for this case. |
| Multiple statements / missing final semicolon | Formatted | Acceptable. |
| Dollar-quoted `DO` body | Mostly passed through | Insufficient layout coverage. |
| Simple join | Added `INNER` and `AS` | Disallowed noisy/semantic-risk rewrite policy. |
| Complex join | Two-space indentation and non-canonical `ON` layout | Style blocker. |
| Authored result groups | Expanded to one item per line | Source-group blocker. |
| `MERGE` | Parse failure | Statement-coverage blocker in the grammar. |
| `CAST(value AS bigint), value != 1` | Rewritten to `value::BIGINT, value <> 1` | Opposite of required lexical policy. |

The relevant implementation is about 7,369 lines of private renderer code.
Inline comments would need changes across expression, select, statement, and
PL/pgSQL paths. Authored line groups are not modeled. `CAST`, operator,
parenthesis, alias, and join rewrites are intentional renderer behavior rather
than one isolated bug.

## Why `[patch.crates-io] libpgfmt` is not the smaller option

Cargo can replace `libpgfmt` with a path or Git fork. The mechanism is not the
problem. A conforming fork would still require:

1. cross-cutting comment preservation;
2. source-line and hard-boundary group metadata in every list renderer;
3. disabling several intentional rewrites;
4. a second patch to the PostgreSQL tree-sitter grammar for `MERGE`;
5. a real PostgreSQL parser for parse-before/after and structural equivalence.

That is a formatter fork, a grammar fork, and a safety parser. The selected
backend has one parser/scanner dependency and leaves only Semantic Block layout
policy in this repository.

Reconsider a `libpgfmt` patch if upstream gains lossless inline comments,
source-aware groups, `MERGE`, configurable lexical rewrites, and strict parsing.

## Safety proof

The formatter facade executes:

1. parse the original with PostgreSQL;
2. scan exact source tokens;
3. apply Semantic Block layout without reordering tokens;
4. parse the formatted output;
5. remove only source-location fields and compare PostgreSQL parse trees;
6. compare strings, quoted identifiers, dollar strings, and comments byte for
   byte and in order;
7. format again and require byte-identical idempotence.

The Batch 1 tests prove:

- invalid PostgreSQL is rejected before layout;
- literal and result-column order changes fail structural equivalence;
- keyword/function/type casing, `<>` to `!=`, and whitespace pass;
- compact `SELECT`, mixed boolean precedence, compact join, canonical expanded
  join, comments, and dollar strings are idempotent.

No other statement or layout coverage is claimed by Batch 1.
