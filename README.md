# semblock

`semblock` is a fast, deterministic PostgreSQL formatter implementing
the **Semantic Block SQL** style.

The repository now has a **runnable CLI MVP**. It formats standalone `.sql`
files and complete PostgreSQL statements inside Go raw backtick strings,
recursively walks projects, respects ignore files, and supports local/CI
`fmt`, `check`, and `diff` workflows.

The current formatter-core coverage is `SELECT`, authored result/function
argument groups, booleans, compact/expanded `CASE`, joins, CTEs, and recursive
CTEs. Broader DML, DDL, `MERGE`, and PL/pgSQL layout remain Batch 3 and are not
claimed without fixtures.

The formatter uses the existing `pg_query` crate for the real PostgreSQL
parser, scanner, token ranges, comments, and AST. Project code implements the
Semantic Block layout policy, not another SQL parser.

## Project documents

- [Core `fmt` / `check` specification](docs/semantic-block-sql-fmt-check-core-spec.md)
- [Formatter design](docs/formatter-design.md)
- [Implementation checklist](docs/implementation-checklist.md)
- [Upstream baseline](docs/upstream-baseline.md)
- [Batch 1 backend spike](docs/batch-1-backend-spike.md)
- [Batch 2 formatter-core MVP](docs/batch-2-core-mvp.md)
- [Runnable CLI and Go MVP](docs/batch-4-5-cli-mvp.md)
- [Technical handoff](docs/semantic-block-sql-work-handoff.md)
- [Russian style guide](docs/semantic-block-sql-style-guide-ru.md)
- [Source artifact provenance](docs/source/README.md)
- [Repository working rules](AGENTS.md)
- [Third-party notices](THIRD_PARTY_NOTICES.md)

The canonical repository-scoped formatting skill is stored in
`.agent-skills/postgresql-sql-format/`. The `.agents/skills/` and
`.claude/skills/` entries are symlinks to that one copy.

## Build and run

Rust 1.88, a C toolchain, and libclang are required because the pinned
`pg_query` backend compiles PostgreSQL's parser.

```bash
cargo build --release
./target/release/semblock --help
```

## Usage

```bash
semblock fmt .
semblock check .
semblock diff .
semblock fmt migrations/001_init.sql
semblock fmt --stdin --filename query.sql
semblock fmt --language go ./internal/...
```

`check` emits stable rule IDs with UTF-8 byte ranges:

```text
query.sql:0-6: error[casing.keyword]: SQL keyword or grammar construct must be `SELECT` instead of `select`
query.sql:10-10: error[spacing.comma]: token spacing does not match the mandatory spacing rule
```

Directory discovery includes `.sql` and `.go` by default. It respects
`.gitignore` and nested `.semblockignore` files. `.semblockignore` uses
gitignore syntax, has higher precedence than ordinary ignore files, and more
nested files win within that level. Hidden paths are skipped. Passing a file
explicitly processes it even when an ignore rule matches it.

Go auto-detection formats only raw backtick literals that:

- belong to a `const`, `var`, regular assignment, or short assignment;
- begin with a supported SQL statement keyword; and
- parse as one or more complete PostgreSQL statements.

Interpreted Go strings and incomplete fragments such as a standalone `WHERE`
clause are skipped. An explicit SQL marker makes a raw literal mandatory and
therefore diagnostic on parse failure.

## Directives

```go
// semblock:file-ignore
package legacy

// semblock:ignore
const legacyQuery = `select vendor_specific_magic(...)`

// semblock:sql
const query = `/* injected */ select id from public.items;`

// language=SQL
const jetbrainsQuery = `select id from public.items;`
```

```sql
-- semblock:file-ignore

-- semblock:off
SELECT vendor_specific_magic(...);
-- semblock:on
```

These directives affect `fmt`, `check`, and `diff`. Nested, unmatched, or
misplaced directives are errors; ignored SQL regions remain byte-identical.

## Configuration

`semblock` searches for `semblock.toml` from the current directory upward.
`--config` selects an explicit file. Unknown fields and unsupported values are
errors.

```toml
dialect = "postgresql"

[format]
semicolon_policy = "preserve"
not_equal_policy = "preserve"
syntax_diagnostics = "parser_available"

[layout]
indent_width = 4
soft_line_width = 120
hard_line_width = 160
preserve_list_groups = true
preserve_blank_lines = true

[discovery]
respect_gitignore = true
ignore_file = ".semblockignore"

[go]
enabled = true
auto_detect = true
raw_strings = true
interpreted_strings = false
```

Exit codes are stable:

| Code | Meaning |
| --- | --- |
| `0` | Success. |
| `1` | `check` or `diff` found formatting changes. |
| `2` | Invalid CLI arguments or configuration. |
| `3` | SQL, directive, or Go parse/validation failure. |
| `4` | Discovery, filesystem, or atomic replacement failure. |

## Development

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
```
