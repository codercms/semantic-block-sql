# Upstream baseline

Status: **CLI MVP dependencies recorded**

Snapshot date: **2026-07-25**

This document records what was verified before formatter implementation. A
version in this table is not automatically an approved dependency. Reverify the
tag, manifest, license, MSRV, and API at the batch where the dependency is
actually introduced.

## Formatter and parser foundations

| Project | Verified upstream state | License | MSRV / edition | Batch 0 conclusion |
| --- | --- | --- | --- | --- |
| [`gmr/libpgfmt`](https://github.com/gmr/libpgfmt) | `v1.3.0`, commit [`c4b8e73`](https://github.com/gmr/libpgfmt/commit/c4b8e7398f344eabaac21a38ea2c590212c53d93) | BSD-3-Clause | Rust 1.88, edition 2024 | Preferred spike backend, but requires internal changes for this style. |
| [`gmr/pgfmt`](https://github.com/gmr/pgfmt) | `v2.2.0`, commit [`f06c9dd`](https://github.com/gmr/pgfmt/commit/f06c9dd6a3166f7c825008d48c36a0b09354b640) | BSD-3-Clause | edition 2024; inherits backend MSRV | Useful reference CLI, not sufficient as the project CLI. |
| [`gmr/tree-sitter-postgres`](https://github.com/gmr/tree-sitter-postgres) | `19.0.0-beta.2`, commit [`10163f8`](https://github.com/gmr/tree-sitter-postgres/commit/10163f867437b3527592624ef1ecb9fca0853971) | BSD-3-Clause | edition 2024 | Grammar is generated from PostgreSQL `REL_19_BETA2`; pin and test because this is a beta grammar line. |
| [`tree-sitter/tree-sitter-go`](https://github.com/tree-sitter/tree-sitter-go) | `0.25.0`, default-branch snapshot [`2346a3a`](https://github.com/tree-sitter/tree-sitter-go/commit/2346a3ab1bb3857b48b29d779a1ef9799a248cd7) | MIT | edition 2021 | Suitable Go CST candidate; runtime compatibility with tree-sitter 0.26 must be compiled and tested. |
| [`BurntSushi/ripgrep` `ignore`](https://github.com/BurntSushi/ripgrep/tree/master/crates/ignore) | `0.4.31`, default-branch snapshot [`f9c05a9`](https://github.com/BurntSushi/ripgrep/commit/f9c05a949d1a0dc8e16dee28ca9605d38611faeb) | Unlicense OR MIT | Rust 1.88 | Preferred traversal candidate; supports `.gitignore`-aware recursive walking and lower-level matchers. |
| [`pganalyze/pg_query.rs`](https://github.com/pganalyze/pg_query.rs) | crate `6.1.1`, source commit [`66eb7be`](https://github.com/pganalyze/pg_query.rs/commit/66eb7becea1a40e315fee3f90197a35a89d20c25), PostgreSQL 17.4 | MIT | edition 2021; no declared MSRV | **Selected Batch 1 backend** for PostgreSQL parsing, scanning, token ranges, and structural AST comparison. |

The versions above were verified from upstream manifests and current commits,
not inferred from the original handoff.

## CLI MVP support crates

| Crate | Selected version | License | Notes |
| --- | --- | --- | --- |
| `clap` | `4.6.4` | MIT OR Apache-2.0 | Selected derive-based CLI; Rust 1.85 upstream MSRV. |
| `ignore` | `0.4.31` | Unlicense OR MIT | Selected project walker and ignore engine; Rust 1.88 upstream MSRV. |
| `tree-sitter` | `0.26.11` | MIT | Selected Go CST runtime; Rust 1.77 upstream MSRV. |
| `tree-sitter-go` | `0.25.0` | MIT | Selected Go grammar. |
| `serde` | `1.0.229` | MIT OR Apache-2.0 | Selected strict configuration data model. |
| `toml` | `1.1.3+spec-1.1.0` | MIT OR Apache-2.0 | Selected configuration parser; Rust 1.85 upstream MSRV. |
| `similar` | `3.1.1` | Apache-2.0 | Selected unified diff implementation; Rust 1.85 upstream MSRV. |
| `tempfile` | `3.27.0` | MIT OR Apache-2.0 | Selected same-directory temporary-file helper; semblock owns validation, permissions, syncing, and persistence policy. |

`rayon` was not added. `--jobs` bounds the parallel `ignore` walker; source
formatting remains deterministic and sequential until performance work proves
another dependency useful.

## Verified `libpgfmt` extension points

The public entry point is effectively:

```rust
pub fn format(sql: &str, style: Style) -> Result<String, FormatError>
```

The public `Style` enum currently has eight variants. Internally:

- `StyleConfig::from_style` maps variants to a small set of layout booleans;
- `Formatter`, `StyleConfig`, and formatter submodules are crate-private;
- statement layout is split across private `expr`, `select`, `stmt`, and
  `plpgsql` modules;
- `Style::PgDump` selects a dedicated renderer, proving that a materially
  different renderer can coexist with the ordinary style-config path.

This means an external `semblock` crate cannot implement a complete
`Style::SemanticBlock` by wrapping the published API. The realistic Batch 1
choices are:

1. contribute the style and required public options upstream;
2. maintain a small, pinned fork while contributing generally useful fixes;
3. vendor the backend only if a fork dependency proves operationally
   unacceptable.

No choice is final in Batch 0.

## Known blockers and risks

### Comment preservation

The current root formatter preserves standalone top-level comments, but its own
source comment states that comments embedded inside statements are not yet
preserved. That conflicts directly with Semantic Block SQL safety and authored
group boundaries. Batch 1 must reproduce this with fixtures before any backend
decision.

### Parse strictness

`libpgfmt::format` accepts some parse trees containing `ERROR` nodes through a
size/structure heuristic because earlier grammar versions rejected valid SQL.
That is pragmatic for a general formatter, but insufficient as the sole safety
gate for `semblock`. We need a documented strict-validation policy against the
current grammar and fixtures for every tolerated exception.

Tree-sitter is a concrete-syntax parser, not the PostgreSQL server analyzer. It
cannot prove schema-level validity or semantic equivalence. The project safety
claim must therefore remain structural and lexical, backed by reparsing,
token/comment invariants, idempotence, and fixture coverage.

### Source-preserving groups

The current API receives source text, but its style configuration has no
soft/hard widths and no model for authored line, blank-line, or comment
boundaries inside lists. Semantic Block layout therefore needs richer formatter
options and source-span-aware grouping, not another boolean preset alone.

### Statement coverage

Upstream documents supported statements and says unsupported statements are
passed through with normalized whitespace. `semblock` must not report such
syntax as formatted support without a fixture and must avoid normalization that
can lose information.

### CLI rewrite behavior

`pgfmt` is a useful reference, but its in-place mode uses direct `fs::write`,
has only basic check/in-place modes, and has no project traversal, config, diff,
Go extraction, directive model, post-format parse, or idempotence gate. The
`semblock` project CLI must be separate.

### Toolchain availability

Batch 1 installed and verified Rust 1.88.0 and Cargo 1.88.0. The exact
`libpgfmt v1.3.0` upstream suite passed unchanged. `pg_query` additionally
requires a C toolchain and libclang while compiling its bundled PostgreSQL
parser.

## Batch 1 verification gate

Completed evidence and the final decision are recorded in
`docs/batch-1-backend-spike.md`.

The original gate was:

- obtain and record the exact `libpgfmt v1.3.0` source tree;
- run its complete upstream test suite on Rust 1.88+;
- add focused characterization fixtures for comments, `MERGE`, complex joins,
  dollar quoting, logical list lines, invalid SQL, and multiple statements;
- inspect whether source spans survive deeply enough to preserve authored
  groups;
- prove a minimal `SemanticBlock` renderer can produce compact `SELECT`,
  complex `WHERE`, and canonical `JOIN ... ON`;
- document upstream contribution, fork, or vendoring with concrete evidence.
