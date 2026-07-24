# Formatter design

Status: **Batch 0 specification**

Last updated: **2026-07-25**

## Purpose

`semblock` is a fast, deterministic PostgreSQL formatter implementing Semantic
Block SQL. It formats standalone SQL, project trees, stdin, and complete SQL
statements embedded in Go raw string literals. Future IDE adapters call the
same engine.

It is a formatter, not a query optimizer, SQL executor, schema analyzer, or
business-semantic grouping engine.

## Requirement precedence

The initial technical handoff remains the detailed source of truth. The current
project request clarifies or overrides it in these places:

- Go interpreted strings are **disabled for MVP**. Raw backtick strings come
  first; interpreted strings require a separately proven
  decode/format/re-encode round trip.
- Default configuration is:

  ```toml
  [go]
  enabled = true
  auto_detect = true
  raw_strings = true
  interpreted_strings = false
  ```

- Function names and type names are lowercase. Parser-recognized SQL constructs
  such as `COALESCE`, `NULLIF`, `FILTER`, `OVER`, and `ARRAY` remain uppercase
  as keyword-like constructs; ordinary calls such as `count`, `now`, and
  `jsonb_build_object` are lowercase.
- The original ZIP is retained as immutable provenance. The unpacked
  `.agent-skills/postgresql-sql-format/` directory is the active canonical
  skill.

Any future ambiguity is recorded here before implementation.

## Non-negotiable invariants

### Semantic safety

Formatting may change:

- whitespace and indentation;
- line breaks;
- SQL keyword and special-value casing;
- PostgreSQL lexical preference `<>` to `!=`;
- comment position only when attachment to the same syntax node is preserved.

Formatting must not:

- add or remove syntax other than insignificant formatting tokens;
- reorder columns, predicates, joins, CTEs, assignments, rows, statements, or
  set-operation branches;
- rename identifiers or aliases;
- add casts or predicates;
- change string, quoted identifier, comment, or dollar-quoted contents;
- remove potentially meaningful parentheses;
- optimize or refactor a query.

### Determinism and idempotence

For all supported input:

```text
format(format(source)) == format(source)
```

The same bytes, configuration, language, and formatter version must produce the
same result independently of traversal order or job count.

### No partial rewrites

A source file is the atomic unit. If any required parse, formatting,
post-format validation, idempotence, host reparse, or write step fails, the
original file remains unchanged.

### Evidence-based support

A PostgreSQL construct is supported only when a fixture demonstrates:

- original parse acceptance;
- expected golden layout;
- formatted parse acceptance;
- idempotence;
- comment and literal preservation where applicable.

## Layout model

The formatter chooses compact or expanded form from syntax, configured widths,
complexity, and authored layout hints.

Default widths:

```toml
[layout]
indent_width = 4
soft_line_width = 120
hard_line_width = 160
preserve_list_groups = true
preserve_blank_lines = true
```

### Width semantics

- Soft width permits a break; it does not force one.
- An authored cohesive group may exceed soft width.
- Hard width requires the nearest safe syntax-boundary break.
- A line may exceed hard width only when the excess is caused by an indivisible
  token, string, quoted identifier, or comment.
- The formatter never splits inside a token.

### Authored group model

Within list-like syntax, original line boundaries are soft group hints. Blank
lines and comments are hard boundaries.

The formatter:

- preserves a hinted group while it is at or below hard width;
- never merges across a hard boundary;
- does not split a group merely for soft width;
- splits an over-hard group at safe AST/CST argument boundaries;
- greedily packs simple arguments from completely one-line input to soft width;
- normally gives independently complex expressions their own line;
- does not infer business meaning.

Group hints require original source spans to survive parsing and layout. This is
a primary backend-selection criterion.

### Connectors do not own empty levels

`ON`, `THEN`, and comparable connector keywords stay attached to their owning
construct where possible.

Canonical expanded join:

```sql
LEFT JOIN match_new.source_links link ON
    link.kp_id = item.kp_id
    AND link.status = 'approved'
    AND (
        link.model_version = current_model.version
        OR link.match_method = 'manual'
    )
```

Canonical action branch:

```sql
WHEN MATCHED THEN UPDATE SET
    title = source.title,
    updated_at = source.updated_at
```

The connector itself does not create another indentation level.

## Architecture

```text
CLI / stdin / future IDE adapters
                |
          application service
      +---------+----------+
      |                    |
 project discovery     host extraction
      |                 (Go CST)
      +---------+----------+
                |
       formatter facade API
                |
   PostgreSQL parse + Semantic Block layout
                |
      validation + idempotence
                |
       diff or atomic rewrite
```

Planned module boundaries:

```text
src/
├── main.rs
├── cli.rs
├── config.rs
├── discover.rs
├── directives.rs
├── diagnostics.rs
├── diff.rs
├── rewrite.rs
├── formatter/
│   ├── mod.rs
│   ├── semantic_block.rs
│   └── validation.rs
└── host/
    ├── mod.rs
    └── go.rs
```

The exact crate layout may change after the upstream spike, but these dependency
directions may not:

- discovery does not know formatter internals;
- Go extraction does not implement SQL layout;
- formatter does not perform filesystem traversal or writes;
- diff does not mutate files;
- rewrite receives fully validated replacement bytes;
- IDE integrations do not contain another formatting engine.

## Formatter facade

The canonical API should evolve around:

```rust
format_sql(
    source: &str,
    options: &FormatOptions,
) -> Result<FormattedSql, FormatDiagnostic>
```

`FormattedSql` should retain enough information for diagnostics and callers,
for example changed/not-changed state and normalized output. It must not write
files.

The API must serve:

- standalone file formatting;
- stdin;
- Go embedded SQL;
- tests;
- future VS Code and IDEA adapters.

The exact Rust types are a Batch 1 design output after evaluating the backend.

## PostgreSQL backend strategy

The preferred direction remains a `libpgfmt` extension with:

```rust
Style::SemanticBlock
```

Batch 0 inspection found that public `Style` alone is not a sufficient extension
point: the formatter and style configuration are private, widths are absent,
and source-preserving grouping/comments need deeper changes. A pinned fork or
upstream patch is therefore expected for the spike.

The decision order is:

1. demonstrate required behavior in a focused upstream-based spike;
2. propose generally useful correctness and extension changes upstream;
3. keep a small pinned fork if release timing or project-specific layout makes
   upstream-only development impractical;
4. vendor only with a documented operational reason;
5. choose an alternative backend only after recording concrete failed
   requirements.

See `docs/upstream-baseline.md`.

## Go extraction

Go source is parsed structurally with `tree-sitter-go` or a better evidenced Go
syntax parser.

Pipeline:

1. parse the complete Go file;
2. locate string literal nodes and exact byte ranges;
3. associate file/declaration directives through syntax-tree ownership;
4. accept explicit SQL markers or run a cheap SQL-prefix filter;
5. validate the decoded candidate as one or more complete PostgreSQL
   statements;
6. format eligible snippets independently;
7. abort the complete file on any mandatory snippet failure;
8. replace byte ranges from end to start;
9. reparse the complete resulting Go file;
10. atomically replace the file.

Cheap classification may use:

```regex
(?is)^\s*(WITH|SELECT|INSERT|UPDATE|DELETE|MERGE|CREATE|ALTER|DROP|DO|CALL|GRANT|REVOKE|TRUNCATE|COMMENT)\b
```

It is never the SQL authority. Incomplete fragments such as a standalone
`WHERE` clause are not formatted.

MVP supports raw backtick literals only. Interpreted strings remain disabled.

## Directives

Planned directives:

```text
// semblock:file-ignore
// semblock:ignore
// semblock:sql
// language=SQL
-- semblock:file-ignore
-- semblock:off
-- semblock:on
```

Rules:

- Go file directives must be in the documented leading-comment region.
- Declaration directives must be structurally attached to the declaration.
- Explicit SQL markers bypass only the cheap prefix classifier, never parsing.
- SQL block-off regions remain byte-identical.
- Nested, unmatched, or misplaced control directives are errors with spans.

The detailed state machine and attachment rules are a Batch 4/5 deliverable
before implementation.

## CLI contract

Commands:

```text
semblock fmt <paths...>
semblock check <paths...>
semblock diff <paths...>
```

Required options:

```text
--config <path>
--stdin
--filename <name>
--language <auto|sql|go>
--jobs <n>
--verbose
--quiet
```

Provisional exit codes:

| Code | Meaning |
| --- | --- |
| `0` | Success; already formatted or formatting completed. |
| `1` | Differences found in `check` or `diff`. |
| `2` | Invalid command line or configuration. |
| `3` | SQL or host-language parse/validation failure. |
| `4` | Discovery, filesystem, or atomic rewrite failure. |

Exit codes become stable when Batch 4 integration tests and CLI documentation
land. Until then, changing them requires updating this table and the checklist.

## Discovery and ignore behavior

The default is recursive discovery of `.sql` and enabled host-language files
while respecting `.gitignore`. `.semblockignore` adds gitignore-compatible
project rules.

Nested ignore semantics and precedence will follow the selected traversal
library only after they are explicitly documented and tested. Hidden-file
behavior must be configured deliberately because the `ignore` crate skips
hidden paths by default.

## Rewrite validation

Standalone SQL:

1. parse original;
2. format;
3. parse formatted;
4. verify no new structural errors and preservation invariants;
5. format again;
6. require byte idempotence;
7. atomically replace only in `fmt`.

Go source:

1. parse original Go;
2. collect eligible literal spans;
3. format and validate every required snippet;
4. apply replacements in descending byte order in memory;
5. parse the complete new Go source;
6. require literal/runtime-content invariants appropriate to raw strings;
7. atomically replace only in `fmt`.

`check` and `diff` never write. `fmt` preserves permissions and newline
convention where practical. Platform-specific atomic replacement semantics need
explicit integration tests.

## Configuration baseline

```toml
dialect = "postgresql"

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

Configuration discovery, precedence, validation, and unknown-key behavior must
be specified and tested in Batch 4.

## MVP non-goals

- query optimization or semantic refactoring;
- schema-aware analysis or database connections;
- query execution;
- arbitrary SQL fragment formatting;
- inferred business-semantic groups;
- every host language;
- interpreted Go strings;
- mandatory visual alignment;
- a new PostgreSQL parser;
- separate formatter engines in IDE plugins.

## Open decisions

- Upstream contribution vs pinned fork vs vendoring of `libpgfmt`.
- Strict validation policy for parse trees containing error nodes.
- Structural equivalence checks beyond reparsing and lexical invariants.
- Exact representation of authored group hints in the layout engine.
- Cross-platform atomic replace behavior.
- Nested `.semblockignore` behavior and precedence.
- Diagnostic output format for editor integration.

No open decision authorizes bypassing the safety invariants.
