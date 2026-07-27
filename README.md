# semblock

`semblock` is a deterministic, fail-safe PostgreSQL formatter and checker implementing the **Semantic Block SQL** style.

It formats supported PostgreSQL syntax structurally, preserves comments and authored logical groups, and leaves unsupported syntax byte-identical with an explicit `syntax.unsupported` diagnostic instead of guessing.

The project is currently an early, usable release. It is suitable for repository-wide formatting experiments and CI enforcement when unsupported statements are treated as diagnostics rather than silently rewritten.

## Features

- `fmt`, `check`, and `diff` commands;
- standalone `.sql` files and complete PostgreSQL statements in Go raw strings;
- recursive project discovery with `.gitignore` and nested `.semblockignore` support;
- atomic project writes: one invalid file prevents every planned rewrite;
- PostgreSQL parsing through the pinned `pg_query` backend;
- stable rule IDs and UTF-8 byte ranges for editor and CI integrations;
- parse-equivalence, protected-token, comment-preservation, and idempotence safety gates.

## Installation

Prebuilt archives are published on the GitHub Releases page for:

| Platform | Artifact target | Runtime baseline |
| --- | --- | --- |
| Windows x64 | `x86_64-pc-windows-msvc` | Windows 10+ |
| Linux x64 | `x86_64-unknown-linux-gnu` | GLIBC 2.34+; tested on RHEL/Rocky 9, Debian 12, and Ubuntu 24.04 |
| macOS Intel | `x86_64-apple-darwin` | macOS 14+ |
| macOS Apple Silicon | `aarch64-apple-darwin` | macOS 14+ |

Download the archive for your platform, verify it against `SHA256SUMS`, extract it, and place `semblock` or `semblock.exe` on `PATH`.

Confirm the installation:

```bash
semblock --version
semblock --help
```

## Usage

Format files in place:

```bash
semblock fmt .
semblock fmt migrations/001_init.sql
```

Check formatting without writing:

```bash
semblock check .
```

Print unified diffs without writing:

```bash
semblock diff .
```

Read one source from standard input:

```bash
semblock fmt --stdin --filename query.sql
```

Process Go files explicitly:

```bash
semblock fmt --language go ./internal/...
```

`check` and `diff` exit with code `1` when formatting changes are required, making both commands suitable for CI.

## Supported inputs

Directory discovery includes `.sql` and `.go` files by default. It respects `.gitignore` and nested `.semblockignore` files. An explicitly named file is processed even when an ignore rule matches it.

Go support formats raw backtick literals that:

- belong to a `const`, `var`, regular assignment, or short assignment;
- begin with a supported SQL statement keyword; and
- contain one or more complete PostgreSQL statements.

Interpreted Go strings and incomplete fragments such as a standalone `WHERE` clause are skipped.

## SQL coverage

The formatter has fixture-backed structural support for substantial PostgreSQL syntax, including:

- `SELECT`, CTEs, recursive and general set operations;
- result, argument, grouping, sorting, pagination, and window layouts;
- booleans, joins, `CASE`, filtered and ordered aggregates;
- top-level `VALUES`;
- `INSERT` with `VALUES`, query sources, `DEFAULT VALUES`, `OVERRIDING`, `RETURNING`, and `ON CONFLICT`;
- `UPDATE ... FROM`, `DELETE ... USING`, and shared DML `WITH`;
- PostgreSQL 17 `MERGE` with matched and not-matched actions;
- multiple, joined, derived, function, lateral, and parenthesized relation sources;
- basic `CREATE TABLE`, feature-rich `CREATE INDEX`, and multi-action `ALTER TABLE`;
- `CREATE VIEW` and `CREATE MATERIALIZED VIEW`.

Important remaining areas include routines, PL/pgSQL, richer table and partition DDL, and several advanced relation-source variants.

A PostgreSQL statement may be valid while still being unsupported by the formatter. In that case, `semblock` preserves the statement and emits `syntax.unsupported`. This is a deliberate safety boundary, not a parser error.

## Directives

Go source directives:

```go
// semblock:file-ignore
package legacy

// semblock:ignore
const legacyQuery = `select vendor_specific_magic(...);`

// semblock:sql
const query = `select id from public.items;`

// language=SQL
const jetbrainsQuery = `select id from public.items;`
```

SQL directives:

```sql
-- semblock:file-ignore
```

```sql
-- semblock:off
SELECT vendor_specific_magic(...);
-- semblock:on
```

Nested, unmatched, or misplaced directives are errors. Ignored regions remain byte-identical.

## Configuration

`semblock` searches for `semblock.toml` from the current directory upward. Use `--config` to select an explicit file. Unknown fields and unsupported values are errors.

```toml
dialect = "postgresql"

[format]
semicolon_policy = "preserve"
not_equal_policy = "preserve"
syntax_diagnostics = "parser_available"

[layout]
soft_line_width = 120
hard_line_width = 160

[discovery]
respect_gitignore = true
ignore_file = ".semblockignore"

[go]
enabled = true
auto_detect = true
raw_strings = true
interpreted_strings = false
```

Indentation is always four spaces. Authored list groups, blank lines, and comment boundaries are mandatory and are not configurable.

## Diagnostics and exit codes

Example diagnostic:

```text
query.sql:0-6: error[casing.keyword]: SQL keyword or grammar construct must be `SELECT` instead of `select`
```

| Code | Meaning |
| --- | --- |
| `0` | Success. |
| `1` | `check` or `diff` found formatting changes. |
| `2` | Invalid CLI arguments or configuration. |
| `3` | SQL, directive, or Go parse/validation failure. |
| `4` | Discovery, filesystem, or atomic replacement failure. |

## Build from source

Requirements:

- Rust 1.88;
- a C/C++ build toolchain;
- CMake;
- Clang and `libclang`.

The native toolchain is required because `pg_query` compiles PostgreSQL parser sources.

```bash
git clone https://github.com/codercms/semantic-block-sql.git
cd semantic-block-sql
cargo build --locked --release
./target/release/semblock --version
```

On Windows, the binary is written to `target\release\semblock.exe`.

## Development

Read [AGENTS.md](AGENTS.md) before changing formatter behavior. The core formatter contract, architecture, extension procedure, and historical implementation notes are indexed in [docs/README.md](docs/README.md).

Run the complete local gate:

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo doc --locked --no-deps
git diff --check
```

New PostgreSQL syntax is added through explicit AST capability records, owned token ranges, golden fixtures, semantic-equivalence checks, and idempotence tests. Unsupported syntax must remain fail-safe.

## Agent skill

The repository contains one canonical, vendor-neutral formatting skill at:

```text
.agent-skills/postgresql-sql-format/
```

Copy that directory into an agent-specific discovery path only when needed. The repository does not maintain duplicate Codex- and Claude-specific copies.

## License

MIT. See [LICENSE](LICENSE) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
