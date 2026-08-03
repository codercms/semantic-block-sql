# semblock

`semblock` is a deterministic, fail-safe PostgreSQL formatter and checker implementing the **Semantic Block SQL** style.

It formats supported PostgreSQL syntax structurally, preserves comments and authored logical groups, and skips opaque statement units byte-identically instead of guessing. Unsupported syntax receives `syntax.unsupported`; formatter safety failures receive `format.statement_skipped`. Strict enforcement is an explicit opt-in.

The project is currently an early, usable release. It is suitable for repository-wide formatting and CI: supported units continue formatting when a parser-proven sibling is unsupported or fails a formatter safety gate, while malformed documents remain fatal.

## Features

- `fmt`, `check`, and `diff` commands;
- standalone `.sql` files and complete PostgreSQL statements in Go raw or interpreted string expressions;
- recursive project discovery with `.gitignore` and nested `.semblockignore` support;
- safe project writes: malformed input or safety failures prevent writes, while valid unsupported units are skipped by default;
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
semblock check --list-different --summary
```

Print unified diffs without writing:

```bash
semblock diff .
```

Read one source from standard input:

```bash
semblock fmt --stdin --filename query.sql
echo 'select 1;' | semblock fmt --stdin --language sql
```

Process Go files explicitly:

```bash
semblock fmt --language go ./internal/...
```

`check` and `diff` exit with code `1` when formatting changes are required, making both commands suitable for CI. Add `--strict-unsupported` when opaque unsupported or safety-skipped statements must fail CI and prevent every project write.

### Project and Git workflows

Inspect or format only paths staged in the current Git repository:

```bash
semblock check --staged
semblock diff --staged
semblock fmt --staged
```

Staged `check` and `diff` inspect the stage-0 blobs stored in the Git index, not
the corresponding worktree files. Staged `fmt` first requires every selected
index blob to match its worktree file byte-for-byte, so partially staged files
are rejected before formatting begins. A successful staged `fmt` changes only
the worktree and never modifies the index or runs `git add`; stage the formatting
changes manually afterward.

Check every live path changed relative to a reference:

```bash
semblock check --changed-since origin/main
```

`--changed-since` uses the merge base with `HEAD` and includes committed,
staged, unstaged, and untracked files while excluding deleted files. Paths
selected through Git still follow normal discovery rules, including
`.gitignore`, nested `.semblockignore` files, hidden-path policy, language
selection, and Go enablement.

Inspect configuration resolution or create a project configuration:

```bash
semblock config path
semblock config show
semblock init
```

## Supported inputs

Directory discovery includes `.sql` and `.go` files by default. It respects `.gitignore` and nested `.semblockignore` files. An explicitly named file is processed even when an ignore rule matches it.

Go support understands complete SQL values in Go string expressions, including:

- raw backtick and interpreted double-quoted literals;
- package and local declarations, assignments, returns, and standalone calls;
- direct or nested function-call arguments, including `defer` and `go` calls;
- struct/composite fields, map values, slice/array elements, and table-driven tests;
- compile-time concatenations made entirely from string literals.

Interpreted strings are decoded according to Go lexical rules before SQL formatting.
One-line results remain interpreted; multiline results become readable raw strings
when lossless, otherwise semblock emits deterministic interpreted escapes. Every
replacement is decoded again and compared with the intended formatted runtime value,
and the complete Go file must reparse. Expressions containing identifiers, calls,
indexing, or other runtime values remain untouched. Import paths, struct tags, build
directives, runes, comments, and incomplete fragments are excluded structurally.
Detection is database-library-neutral: PostgreSQL parsing, not a method-name allowlist,
decides whether a candidate is complete SQL.

### Real Go corpus

The normal test suite is fully offline and includes a multi-package golden Go project.
An opt-in network corpus pins permissively licensed releases of go-sqlmock, sqlx, and
pgx, formats selected real source files, runs `gofmt`, verifies a second semblock pass
is byte-idempotent, executes selected `go test` commands, and writes candidate/format/
unsupported/safe-skip metrics:

```bash
./scripts/test-go-corpus.sh --keep
```

```powershell
.\scripts\test-go-corpus.ps1 --keep
```

The resolved commit SHA for every pinned release is recorded in
`target/go-corpus-report.json`. This opt-in runner is not executed by normal CI.

## SQL coverage

The formatter has fixture-backed structural support for substantial PostgreSQL syntax, including:

- `SELECT`, `SELECT INTO`, recursive/general set operations, data-modifying CTEs, `SEARCH` / `CYCLE`, and every PostgreSQL row-lock strength;
- result, argument, grouping, sorting, pagination, window, boolean, `CASE`, filtered aggregate, and ordered aggregate layouts;
- top-level `VALUES`, scalar and predicate subqueries, and subqueries in reviewed `UPDATE`, `DELETE`, and `MERGE` expressions;
- `INSERT` with query sources, `DEFAULT VALUES`, `OVERRIDING`, `RETURNING`, and `ON CONFLICT`;
- `UPDATE ... FROM`, `DELETE ... USING`, and PostgreSQL 17 `MERGE` with matched and not-matched actions;
- multiple, joined, derived, function, lateral, parenthesized, `ROWS FROM`, and `TABLESAMPLE` relation sources, including alias column/definition lists and derived queries containing `WITH`;
- `DROP`, `TRUNCATE`, object and role `GRANT` / `REVOKE`, and `COMMENT ON`;
- top-level `BEGIN` with reviewed transaction modes and unchained `COMMIT`,
  including `WORK` / `TRANSACTION` spellings;
- enum/composite types, domains, sequences, triggers, and row-security policies;
- `CREATE TABLE` with inheritance, typed tables, access/storage/tablespace/on-commit options, partition keys, `PARTITION OF`, and range/list/hash/default partition bounds;
- feature-rich `CREATE INDEX`, multi-action `ALTER TABLE`, `CREATE VIEW`, and `CREATE MATERIALIZED VIEW`;
- reviewed operational and migration utilities: `COPY` (including protected `FROM STDIN` payloads), `CALL`, `EXPLAIN`, `VACUUM`, `ANALYZE`, `REFRESH MATERIALIZED VIEW`, `LISTEN`, `NOTIFY`, extension/schema/statistics/collation/cast creation, and reviewed `ALTER TYPE` / domain / policy / rename forms;
- parser-backed PL/pgSQL declarations, SQL statements, conditionals, exception handlers, loops, `FOREACH`, procedural `CASE`, dynamic `EXECUTE`, cursor operations, `ASSERT`, `RETURN QUERY`, compact bodies, and reviewed `EXIT` / `CONTINUE` forms.

Important remaining areas include advanced SQL/JSON (`JSON_TABLE`, SQL-standard JSON query/value/aggregate forms), `CREATE TABLE AS` / `LIKE`, `XMLTABLE`, and procedural transaction control. PL/pgSQL transaction statements are preserved as unsupported units by default while supported siblings continue formatting.

A PostgreSQL statement may be valid while still being unsupported by the formatter, or a nominally supported statement may fail an ownership or safety gate. By default, `semblock` preserves only that statement byte-for-byte, emits `syntax.unsupported` or `format.statement_skipped`, and continues formatting supported siblings in the same file or project. The skipped-statement diagnostic includes the statement's starting line. Set `format.unsupported_policy = "error"` or pass `--strict-unsupported` to elevate either opaque outcome to an error and retain project-wide no-write preflight. Malformed documents remain fatal because their statement boundaries are not trustworthy.

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
unsupported_policy = "skip"

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
interpreted_strings = true
multiline_string_style = "prefer_raw"
```

Indentation is always four spaces. Authored list groups, blank lines, and comment boundaries are mandatory and are not configurable.

## Diagnostics and exit codes

Example diagnostic:

```text
query.sql:1:1 (bytes 0-6): error[casing.keyword]: SQL keyword or grammar construct must be `SELECT` instead of `select`
```

CLI diagnostics use one-based `line:column` locations for editor and terminal
navigation. The matching half-open UTF-8 byte range follows in parentheses for
exact tooling integration. Coordinates always refer to the source a user can
act on:

- `check` and `diff` refer to the original input and never rewrite it;
- successful `fmt` refers to the final formatted file;
- successful `fmt --stdin` refers to formatted stdout;
- fatal or strict-policy `fmt` failures refer to the unchanged input.

The reusable source API exposes both views without offset-delta tracking:
`FormattedSource::diagnostics` is input-relative, and
`FormattedSource::output_diagnostics` is relative to `FormattedSource::output`.
The output view is collected from the existing idempotence pass, so it does not
require another parse.

| Code | Meaning |
| --- | --- |
| `0` | Success. |
| `1` | `check` or `diff` found formatting changes. |
| `2` | Invalid CLI arguments or configuration. |
| `3` | SQL, directive, or Go parse/validation failure, or unsupported syntax under explicit strict policy. |
| `4` | Discovery, filesystem, or atomic replacement failure. |

## Build from source

Requirements:

- Rust 1.88;
- a C/C++ build toolchain;
- CMake;
- Clang and `libclang`;
- Go 1.22+ when running the complete test suite.

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

New PostgreSQL syntax is added through explicit AST capability records, owned token ranges, golden fixtures, semantic-equivalence checks, and idempotence tests. Unsupported syntax must remain byte-identical and non-fatal by default; strict unsupported policy is an explicit project choice.

## Agent skill

The repository contains one canonical, vendor-neutral formatting skill at:

```text
.agent-skills/postgresql-sql-format/
```

Copy that directory into an agent-specific discovery path only when needed. The repository does not maintain duplicate Codex- and Claude-specific copies.

## License

MIT. See [LICENSE](LICENSE) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
