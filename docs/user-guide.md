# User guide

This guide contains operational details intentionally kept out of the project README. The authoritative formatter behavior is defined by the [core `fmt` / `check` specification](semantic-block-sql-fmt-check-core-spec.md).

## Commands

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

`check` and `diff` return exit code `1` when formatting changes are required.

### Standard input

Read one source from stdin:

```bash
semblock fmt --stdin --filename query.sql
echo 'select 1;' | semblock fmt --stdin --language sql
```

When `--language auto` is used with stdin, `--filename` is required so semblock can infer the source language and produce useful diagnostics.

## Project discovery

Directory discovery includes `.sql` and `.go` files by default.

Normal discovery:

- respects `.gitignore`;
- respects nested `.semblockignore` files;
- applies language selection and Go enablement;
- processes an explicitly named file even when an ignore rule would otherwise match it.

Use `--jobs` to control discovery and formatting concurrency.

## Git workflows

### Staged files

```bash
semblock check --staged
semblock diff --staged
semblock fmt --staged
```

Staged `check` and `diff` inspect the stage-0 blobs stored in the Git index, not the corresponding worktree files.

Staged `fmt` first requires each selected index blob to match its worktree file byte-for-byte. Partially staged files are therefore rejected before formatting begins. A successful staged `fmt` changes only the worktree: it never modifies the index and never runs `git add`.

### Changed since a reference

```bash
semblock check --changed-since origin/main
```

`--changed-since` uses the merge base with `HEAD` and selects live paths changed relative to that base. It includes committed, staged, unstaged, and untracked files and excludes deleted files.

Paths selected through Git still follow normal discovery rules.

## Unsupported syntax and strict mode

A PostgreSQL statement can be parser-valid while still being outside semblock's reviewed formatter capability set.

By default:

- `syntax.unsupported` means the statement is valid PostgreSQL but outside the reviewed syntax boundary;
- `format.statement_skipped` means the statement is nominally supported but formatter ownership or a safety gate could not be proven;
- either case preserves that statement byte-for-byte;
- supported sibling statements continue formatting.

When a skipped-statement failure has a trusted token location, its diagnostic
points to that exact cause while the complete statement is still preserved.
Failures without a reliable location use the complete statement range.

Trailing Unicode whitespace on any physical comment line is ordinary fixable
`spacing.trailing_whitespace`; it is removed without changing comment
attachment or causing a statement skip.

Make either condition fatal with:

```bash
semblock check --strict-unsupported .
```

or in `semblock.toml`:

```toml
[format]
unsupported_policy = "error"
```

Malformed documents remain fatal because their statement boundaries are not trustworthy.

## Go source

Go support understands complete SQL values in string expressions, including:

- raw backtick and interpreted double-quoted literals;
- package and local declarations;
- assignments and returns;
- standalone, direct, and nested function-call arguments, including `defer` and `go` calls;
- struct/composite fields;
- map values;
- slice and array elements;
- table-driven tests;
- compile-time concatenations made entirely from string literals.

Detection is database-library-neutral. PostgreSQL parsing, rather than a method-name allowlist, decides whether a candidate is a complete SQL statement.

### Interpreted and raw strings

Interpreted strings are decoded according to Go lexical rules before SQL formatting.

After formatting:

- one-line interpreted strings remain interpreted;
- multiline results become readable raw strings when the runtime value can be preserved losslessly;
- otherwise semblock emits deterministic interpreted escapes.

Every replacement is decoded again and compared with the intended formatted runtime value, and the complete Go file must reparse.

Expressions containing identifiers, calls, indexing, or other runtime values remain untouched. Import paths, struct tags, build directives, runes, comments, and incomplete fragments are excluded structurally.

By default, semblock preserves a complete Go file byte-for-byte when comments before the package clause contain both `generated` and `DO NOT EDIT`, matched case-insensitively. The phrases may be in one comment or split across the leading comments, covering Go's standard marker and variants emitted by generators such as Swag and Hero. Set `ignore_generated_files = false` under `[go]` to process generated files. Auto-detected string values that are not valid UTF-8 are never SQL candidates; an explicit SQL marker still reports them as invalid.

Process Go explicitly with:

```bash
semblock fmt --language go ./internal/...
```

## Directives

### Go directives

Ignore a complete file:

```go
// semblock:file-ignore
package legacy
```

Ignore one candidate:

```go
// semblock:ignore
const legacyQuery = `select vendor_specific_magic(...);`
```

Force SQL interpretation:

```go
// semblock:sql
const query = `select id from public.items;`

// language=SQL
const jetbrainsQuery = `select id from public.items;`
```

### SQL directives

Ignore a complete file:

```sql
-- semblock:file-ignore
```

Disable formatting for a region:

```sql
-- semblock:off
SELECT vendor_specific_magic(...);
-- semblock:on
```

Ignored source remains byte-identical. Nested, unmatched, or misplaced directives are errors.

## Configuration

Semblock searches for `semblock.toml` from the current directory upward. Use `--config` to select an explicit file.

Generate the default configuration:

```bash
semblock init
```

Inspect configuration resolution:

```bash
semblock config path
semblock config show
```

Unknown fields and unsupported values are errors.

Default configuration shape:

```toml
dialect = "postgresql"

[format]
semicolon_policy = "preserve"
not_equal_policy = "preserve"
syntax_diagnostics = "parser_available"
unsupported_policy = "skip"

[format.type_aliases]

[layout]
soft_line_width = 120
hard_line_width = 160

[discovery]
respect_gitignore = true
ignore_file = ".semblockignore"

[go]
enabled = true
auto_detect = true
ignore_generated_files = true
raw_strings = true
interpreted_strings = true
multiline_string_style = "prefer_raw"
```

Indentation is always four spaces. Authored list groups, blank lines, and comment boundaries are mandatory structural boundaries and are not configurable.

### Type-alias preferences

`[format.type_aliases]` is empty by default, so authored type spelling is
preserved. Set only families the project wants to normalize. `check` reports a
fixable `type.alias` diagnostic and `fmt` applies the preferred spelling.

```toml
[format.type_aliases]
integer = "int"
character_varying = "varchar"
timestamp_with_time_zone = "timestamptz"
```

| Family key | Accepted preferences |
| --- | --- |
| `smallint` | `smallint`, `int2` |
| `integer` | `integer`, `int`, `int4` |
| `bigint` | `bigint`, `int8` |
| `smallserial` | `smallserial`, `serial2` |
| `serial` | `serial`, `serial4` |
| `bigserial` | `bigserial`, `serial8` |
| `boolean` | `boolean`, `bool` |
| `character` | `character`, `char` |
| `character_varying` | `character varying`, `varchar` |
| `bit_varying` | `bit varying`, `varbit` |
| `numeric` | `numeric`, `decimal` |
| `real` | `real`, `float4` |
| `double_precision` | `double precision`, bare `float`, `float8` |
| `time_with_time_zone` | `time with time zone`, `timetz` |
| `timestamp_without_time_zone` | `timestamp`, `timestamp without time zone` |
| `timestamp_with_time_zone` | `timestamp with time zone`, `timestamptz` |

Unknown keys and values are configuration errors. Modifiers and arrays are
preserved, and `float(p)`, quoted, qualified, and custom type names are not
rewritten. The checked-in
[all-aliases example](../examples/semblock-all-type-aliases.toml) enables every
family.

`varchar` and `text` are separate PostgreSQL types, not aliases, so the
formatter never converts between them. Opting into unqualified shorthand names
also assumes the project does not shadow PostgreSQL built-in type names through
`search_path`.

## Diagnostics

Example:

```text
query.sql:1:1 (bytes 0-6): error[casing.keyword]: SQL keyword or grammar construct must be `SELECT` instead of `select`
```

CLI diagnostics use one-based `line:column` locations followed by a half-open UTF-8 byte range.

Coordinates refer to the source the user can act on:

- `check` and `diff` refer to the original input;
- successful `fmt` refers to the final formatted file;
- successful `fmt --stdin` refers to formatted stdout;
- fatal or strict-policy `fmt` failures refer to the unchanged input.

The Rust source API exposes both views: `FormattedSource::diagnostics` is input-relative and `FormattedSource::output_diagnostics` is relative to `FormattedSource::output`.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success. |
| `1` | `check` or `diff` found formatting changes. |
| `2` | Invalid CLI arguments or configuration. |
| `3` | SQL, directive, or Go parse/validation failure, or unsupported syntax under explicit strict policy. |
| `4` | Discovery, filesystem, or atomic replacement failure. |

## Further reading

- [PostgreSQL coverage](sql-coverage.md)
- [Semantic Block SQL style guide](semantic-block-sql-style-guide-ru.md)
- [Core `fmt` / `check` specification](semantic-block-sql-fmt-check-core-spec.md)
- [Project documentation index](README.md)
