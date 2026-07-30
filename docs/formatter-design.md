# Formatter design

Status: **Runnable CLI and raw-Go MVP complete**

Last updated: **2026-07-29**

## Purpose

`semblock` is a fast, deterministic PostgreSQL formatter implementing Semantic
Block SQL. It formats standalone SQL, project trees, stdin, and complete SQL
statements embedded in Go raw string literals. Future IDE adapters call the
same engine.

It is a formatter, not a query optimizer, SQL executor, schema analyzer, or
business-semantic grouping engine.

## Requirement precedence

`docs/semantic-block-sql-fmt-check-core-spec.md` is the authoritative contract
for formatter and style-checker behavior. The older handoff and style guide are
historical design inputs where they do not conflict with that specification.
The repository agent skill guides human/agent formatting but does not define a
second machine contract.

The current core specification supersedes earlier project decisions in these
places:

- terminal semicolons are controlled by `preserve` (default), `require`, or
  `omit`;
- `<>` is preserved by default and may become `!=` only under
  `not_equal_policy = prefer_bang`;
- the exact uppercase built-in whitelist includes `COUNT`, `SUM`, `AVG`,
  `MIN`, `MAX`, `COALESCE`, `NULLIF`, `GREATEST`, `LEAST`, `NOW`, and
  `EXTRACT`; all other unquoted functions remain lowercase;
- four-space indentation, authored-group preservation, and blank-line/comment
  boundaries are mandatory core behavior rather than optional style switches;
- `check` must return rule-level diagnostics with ranges and fix metadata;
- parse or unsupported-format failures return the original source unchanged
  with diagnostics instead of partial output.

The runnable CLI and raw-Go MVP predate this contract and are being reconciled
batch by batch. Exact built-in casing, contextual `INTERVAL`, terminal-semicolon
policy, default `<>` preservation, and final-newline preservation are now
implemented. Rule-level diagnostics, fail-safe formatting results, mandatory
four-space
indentation, and mandatory authored boundaries are also available. Complete
alternative-layout preservation remains to be reconciled. Explicit support
classification now prevents unimplemented syntax from falling through to generic
token normalization.

Current application-level decisions are:

- Go interpreted strings are enabled after a complete decode/format/re-encode
  round trip and runtime-value verification were implemented.
- Multiline interpreted SQL prefers a raw literal when lossless and otherwise
  uses deterministic interpreted escaping.
- Default configuration is:

  ```toml
  [go]
  enabled = true
  auto_detect = true
  raw_strings = true
  interpreted_strings = true
  multiline_string_style = "prefer_raw"
  ```

- The original ZIP is retained as immutable provenance. The unpacked
  `.agent-skills/postgresql-sql-format/` directory is the active canonical
  skill.

Any future ambiguity is recorded here before implementation.

## Architecture hardening decisions

The formatter uses a closed, compiler-checked ownership model rather than a
runtime statement registry. `StatementSpec` carries the exact AST-validated
capabilities for each supported top-level statement. Token binders must verify
those capabilities before producing `StatementLayout` records.

This resolves four earlier architecture risks:

- validation and layout can no longer independently claim different clause
  shapes without producing an ownership safety failure;
- statement token ranges are always half-open, with the terminal semicolon
  stored separately;
- nested SELECT/VALUES validation uses deterministic protobuf shape checks
  rather than in-memory pointer identity;
- layout binding, statement planning, list planning, rendering, and semantic
  equivalence now live in focused modules instead of two growing monoliths.

Individual clause positions remain indices into the one immutable scanner-token
slice. Only ranges are wrapped because boundary semantics are the realistic
source of index bugs; capability/cardinality verification protects clause
positions without introducing pervasive conversion wrappers.

The implemented architecture and extension workflow are documented in
[`formatter-architecture.md`](formatter-architecture.md) and
[`formatter-extension-guide.md`](formatter-extension-guide.md).

Relation-bearing DML and MERGE statements now share one recursive source
capability model. Validation records ordinary relations, SELECT-derived tables,
simple set-returning functions, and join trees; token binding must reproduce the
same top-level source kinds and join predicate cardinalities. `CREATE VIEW` and
`CREATE MATERIALIZED VIEW` use separate capability records so view suffixes and
materialized-view population clauses cannot leak into SELECT ownership.

## Non-negotiable invariants

### Semantic safety

Formatting may change:

- whitespace and indentation;
- line breaks;
- SQL keyword and special-value casing;
- PostgreSQL `<>` to `!=` only under the configured `prefer_bang` policy;
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
soft_line_width = 120
hard_line_width = 160
```

Indentation is fixed at four spaces. Authored list groups, blank lines, and
comment boundaries are mandatory formatter invariants rather than options.

### Cast, array, and JSON expression punctuation

Fixture-backed scalar-expression rendering now treats PostgreSQL type and array
syntax as owned punctuation rather than generic binary-token spacing:

- casts render as `value::type` or `CAST(value AS type)`;
- unquoted type names are lowercase, including qualified custom names and
  contextual multiword types such as `double precision`, `character varying`,
  and `timestamp WITH time zone`;
- quoted type-name components remain byte-identical;
- array constructors, array type suffixes, subscripts, and slices are tight:
  `ARRAY[1, 2]`, `text[]`, `items[1]`, and `items[1:3]`;
- `ARRAY (SELECT ...)` remains grammar-parenthesis syntax and is not folded into
  square-bracket ownership;
- legacy PostgreSQL JSON operators (`->`, `->>`, `#>`, `#>>`, `#-`, `@>`, `<@`,
  `?`, `?|`, `?&`, and `||`) render as binary operators with one surrounding
  space while preserving JSON and path literals exactly.

Simple `JSON_OBJECT(key: value)` and `JSON_ARRAY(value)` constructors retain
existing fixture-backed support. Advanced SQL/JSON constructors and query,
aggregate, parse, scalar, serialization, predicate, and table expression roots
fail closed with `syntax.unsupported` until their complete grammar clauses have
owned layouts and acceptance fixtures.

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

Batch 2 implements this model for `SELECT` result lists and parenthesized
function-argument lists. Scanner gap metadata records authored line and blank
line boundaries without adding another parser. Completely one-line lists that must expand are rendered one item per line;
authored groups are retained through soft width and split only at comma
boundaries when hard width requires it.

Comments remain in the scanner token order. Line comments always retain a
physical line ending so they cannot consume a following token. Blank lines and
standalone comments remain hard list boundaries when their preservation option
is enabled.

### Hard-width result

After layout, the formatter validates every output line. A remaining
over-hard, breakable line is an error rather than silently violating the
contract. When an indivisible string, quoted identifier, identifier, or comment
necessarily exceeds hard width, formatting succeeds and returns
`FormatWarning::IndivisibleTokenExceedsHardWidth { line, width }`.

`FormatOptions` exposes:

```rust
FormatOptions {
    style,
    soft_line_width,
    hard_line_width,
    semicolon_policy,
    not_equal_policy,
    syntax_diagnostics,
}
```

The core-policy defaults are `semicolon_policy = preserve`,
`not_equal_policy = preserve`, and `syntax_diagnostics = parser_available`.
Formatting policies are applied by the token-preserving renderer, while parser,
scanner, safety-gate, and hard-width failures are exposed through the shared
diagnostic result model without returning partial output.

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
├── diff.rs
├── rewrite.rs
├── formatter/
│   ├── diagnostics.rs
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

The specification-facing APIs are fail-safe value results:

```rust
format_sql_result(
    source: &str,
    options: &FormatOptions,
) -> FormatResult

check_sql(
    source: &str,
    options: &FormatOptions,
) -> CheckResult
```

`FormatResult` contains `output`, `changed`, and rule-level diagnostics. Parse,
scan, equivalence, idempotence, and hard-width failures retain the original
source and return a non-fixable diagnostic. `CheckResult` uses the same analysis
and is compliant only when no formatting change or error diagnostic exists.

The older strict `format_sql(...) -> Result<FormattedSql, FormatDiagnostic>`
entry point remains for internal application layers that must abort a complete
file or Go host rewrite. Its successful result now carries the same diagnostics
plus the legacy width-warning field. It must not write files.

Each diagnostic carries:

```text
rule_id
severity
message
source_range   # UTF-8 byte range in the original source
fix_available
```

SQL directive ranges are shifted to document offsets, CRLF normalization is
mapped back to original byte offsets, and Go-host diagnostics are conservatively
attributed to the complete owning raw literal until exact envelope mapping is
implemented.

The API must serve:

- standalone file formatting;
- stdin;
- Go embedded SQL;
- tests;
- future VS Code and IDEA adapters.

The facade remains pure and performs no filesystem writes. The CLI renders
successful style diagnostics as `path:start-end: severity[rule_id]: message`;
`fmt` and `diff` suppress fixed style errors but still surface warnings.

## Code architecture

The implemented parser, ownership IR, token model, layout planner, writer,
extension protocol, and forward-compatibility behavior are documented in
[`formatter-architecture.md`](formatter-architecture.md). The architecture
document describes code structure; this design document remains the record of
behavioral decisions.

## PostgreSQL backend strategy

Batch 1 rejected a `libpgfmt` fork after running its unmodified tests and a
focused characterization suite. The blockers are cross-cutting inline-comment
loss, intentional disallowed rewrites, no authored-group model, permissive
`ERROR` recovery, and missing `MERGE` grammar support. A Cargo patch would also
need a grammar patch and a separate safety parser.

The selected MVP backend is exactly pinned `pg_query 6.1.1`. It supplies the
real PostgreSQL 17.4 parser, scanner, token ranges, comments, and protobuf AST.
Semantic Block layout is the only project-specific formatting layer.

The formatter validates canonical PostgreSQL parse-tree equality after removing
only source-location fields. It separately compares protected token text and
order, then requires a byte-identical second formatting pass.

Before layout, the same PostgreSQL AST is classified against the fixture-backed
support boundary. Unsupported statement families, unowned clauses, advanced
aggregate/window forms, lateral or derived sources, and unknown future protobuf
shapes return `syntax.unsupported` over the original statement range. General
set operations and the recursive CTE `UNION ALL` shape are now supported through
owned branch records. The classifier is extended in the same batch that
introduces each new statement planner; generic token normalization is never the
fallback for unsupported syntax.

The INSERT planner owns VALUES, source SELECT, DEFAULT VALUES, OVERRIDING,
RETURNING, and fixture-backed ON CONFLICT. Column lists, individual VALUES rows,
RETURNING expressions, conflict targets, and `DO UPDATE SET` assignments share
the source-aware list planner: short forms stay compact, authored groups remain
stable, width-driven ungrouped lists expand one item per line, and complex rows
may expand independently. `ON CONFLICT DO NOTHING` may remain compact; `DO
UPDATE` separates the conflict target, action, `SET`, and action `WHERE`. A
conflict-target predicate remains owned by ON CONFLICT, while the later
predicate remains owned by the update action. SELECT-backed WITH clauses reuse
the same `WithBlock` for SELECT, INSERT, UPDATE, DELETE, and MERGE.

The UPDATE planner supports a target relation, simple named assignments,
optional one-relation `FROM`, `WHERE`, and `RETURNING`. Statement-level
expansion separates UPDATE clauses, but does not force locally compact `SET` or
`RETURNING` lists to expand. Each list uses its own authored groups, complexity,
and width: authored groups remain stable while they fit the hard limit, and a
list that requires expansion breaks only at safe item boundaries. `WITH`,
`ONLY`, multi-column or subscripted assignment targets, multiple or joined FROM
sources, and subqueries remain fail-safe unsupported shapes.

The MERGE planner is a separate exhaustive statement variant because its branch
ownership is grammar-specific. `MergeBlock` owns USING/ON, ordered WHEN
branches, and RETURNING; each `MergeBranch` owns its optional condition and a
closed `MergeAction` variant for DELETE, UPDATE SET, INSERT VALUES, or DO
NOTHING. Blank lines separate branches, action introducers stay on the owner
line, and existing list/predicate planners format nested assignments and values.
The current safe subset accepts plain target/source relations and preserves
derived or joined sources unchanged with `syntax.unsupported`.

The DELETE planner supports a target relation, an optional single plain `USING`
relation, `WHERE`, and `RETURNING`. Compact DELETE statements remain inline;
`USING`, authored layout, width, or a complex predicate expands subsequent
clauses at statement scope. `WITH`, `ONLY`, multiple or joined USING sources,
derived sources, and subqueries remain fail-safe unsupported shapes.

See `docs/batch-1-backend-spike.md` for evidence and the dependency update policy.

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

### Embedded Go indentation precedence

The 2026-07-29 style-guide 1.0.1 requirement supersedes the earlier MVP
envelope behavior: multiline SQL in Go raw strings is formatted at SQL root
indentation, independent from the surrounding Go block. The closing backtick
retains its authored host indentation, the original LF or CRLF convention is
preserved, and the complete rewritten Go file is reparsed. A compact one-line
raw string may expand to multiple SQL-root lines without adding host-language
indentation to the SQL body.

### Supported Go string expressions and contexts

The extractor owns expression-level raw literals, interpreted literals, and
literal-only static concatenations. Eligible contexts include declarations,
assignments, returns, direct and nested call arguments, standalone calls,
`defer`, `go`, and composite-literal values such as struct fields, map values,
slice/array elements, and table-driven test cases.

Detection is independent from database package or function names. Import paths,
struct tags, build directives, runes, comments, and incomplete fragments are
excluded structurally. Runtime-dependent concatenations remain byte-identical;
an explicit SQL marker reports them through the default/strict unsupported
policy while supported sibling expressions continue formatting.

### Go project integration evidence

`tests/go_project_integration.rs` copies fixture projects to temporary
directories and invokes the compiled `semblock` CLI. The successful project
proves deterministic `--jobs 1` versus `--jobs 4` output, adjacent byte-for-byte
goldens, clean-check behavior, idempotence, `gofmt -l`, `go test ./...`, and
CRLF preservation. Separate invalid-SQL, invalid-Go, and directive-error
projects prove whole-project preflight: one failure prevents every file write.
The fixture modules use only the Go standard library and require no dependency
downloads.

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

The CLI MVP implements the state machine above. SQL control directives must
occupy their own lines. Go declaration directives are attached to supported
CST owners through adjacent comment nodes. Unmatched, nested, conflicting, or
misplaced directives fail the complete source without a write.

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

Stable CLI exit codes:

| Code | Meaning |
| --- | --- |
| `0` | Success; already formatted or formatting completed. |
| `1` | Differences found in `check` or `diff`. |
| `2` | Invalid command line or configuration. |
| `3` | SQL or host-language parse/validation failure. |
| `4` | Discovery, filesystem, or atomic rewrite failure. |

Changing these codes requires an explicit compatibility decision and updated
integration tests.

## Discovery and ignore behavior

The default is recursive discovery of `.sql` and enabled host-language files
while respecting `.gitignore`. `.semblockignore` adds gitignore-compatible
project rules.

Discovery uses `ignore 0.4.31`. Custom `.semblockignore` files have higher
precedence than ordinary ignore files; more deeply nested custom files win
within that level. `.gitignore` is enabled by default for every traversed
directory tree, including trees that are not Git repositories. Hidden paths and
symlink traversal are disabled. An explicit file argument bypasses directory
ignore matching and is processed.

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

Four-space indentation and authored group, blank-line, and comment-boundary
preservation are fixed core behavior. Obsolete configuration keys for those
rules are rejected by strict TOML parsing.

Configuration starts with built-in defaults, then applies the first
`semblock.toml` found from the current directory upward. `--config` replaces
that search with an explicit path. Unknown keys, invalid widths, unsupported dialects, path-like ignore filenames,
and invalid policy values are configuration errors.

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

- additional fixture-backed PostgreSQL families selected from real project diagnostics;
- Windows/macOS atomic replacement verification beyond the Unix integration
  gate;
- machine-readable diagnostic output for editor integration;
- a proven interpreted-Go-string decode/format/re-encode round trip;
- formatting-worker parallelism after measurement (project discovery is
  already bounded by `--jobs`).

No open decision authorizes bypassing the safety invariants.

## Expanded PostgreSQL capability boundary

The ordinary statement validator now admits a closed set of common migration
utilities in addition to the layout-bearing query/DML/DDL families. `DROP`,
`TRUNCATE`, object and role `GRANT` / `REVOKE`, `COMMENT ON`, enum/composite
`CREATE TYPE`, domains, sequences, triggers, and policies are represented by
`UtilityStatementKind`. The validator checks the exact PostgreSQL AST shape
before the shared token renderer may normalize casing and spacing; unknown
object kinds or option combinations still return `syntax.unsupported`.

Query ownership covers `SELECT INTO`, every row-lock strength and wait policy,
data-modifying CTEs, `SEARCH` / `CYCLE`, and reviewed scalar/predicate subqueries
inside UPDATE, DELETE, and MERGE expressions. Relation-source capabilities cover
`ROWS FROM`, `TABLESAMPLE` / `REPEATABLE`, alias column or definition lists, and
derived queries containing `WITH`. These are AST capabilities consumed by the
existing query and relation binders, not document-wide keyword scans.

Validated CTE body statements remain part of the typed ownership tree instead
of being discarded after the support gate. Nested INSERT, UPDATE, DELETE, and
MERGE bodies are bound to their CTE token ranges and dispatched through the same
statement planners as top-level DML. SELECT bodies continue through the shared
query planner. Predicate subqueries carry a layout indent separately from their
scanner depth so each enclosing IN, EXISTS, ANY, or ALL block remains visible
without weakening structural token matching.

`CREATE TABLE` validation now owns inheritance, typed tables, access methods,
storage parameters, tablespaces, on-commit modes, partition keys, `PARTITION
OF`, and range/list/hash/default partition bounds. Adjacent syntax such as
`CREATE TABLE AS`, `LIKE`, and `XMLTABLE` remains explicitly unsupported.

## Parser-backed PL/pgSQL routine bodies

`DO` blocks and dollar-quoted PL/pgSQL function/procedure bodies use a dedicated
nested parser boundary. `parse_plpgsql` nodes are adapted into typed capability
categories and scanner-bound source spans, then rendered through a procedural
layout IR independent from authored line boundaries. Compact bodies, declarations,
blocks, SQL statements, assignments, conditionals, loops, `FOREACH`, procedural
`CASE`, dynamic execution, cursor operations, diagnostics, exceptions, `ASSERT`,
and reviewed `RETURN QUERY` forms are covered. Transaction control remains opaque
and follows the default/strict unsupported policy.

## Statement-granular unsupported policy

The formatter classifies every parser-proven top-level statement independently. A supported statement is formatted through the normal validation, ownership, equivalence, protected-token, and idempotence gates. A valid but unsupported statement is copied byte-for-byte and receives `syntax.unsupported`. The default `UnsupportedPolicy::Skip` reports that diagnostic as a warning and continues with supported siblings. `UnsupportedPolicy::Error` collects all unsupported statements, elevates them to errors, and returns the complete original document unchanged. PostgreSQL parse errors and formatter safety failures remain fatal under both policies.

`COPY ... FROM STDIN` is split into an AST-validated header plus a protected payload ending at `\.`. Only the header is formatted; payload bytes are never scanned or rewritten.

## Typed PL/pgSQL semantic and layout IR

Routine bodies are no longer formatted from authored lines. `parse_plpgsql` is adapted into typed parser capability categories, while PostgreSQL scanner tokens are bound into a span-bearing `RoutineBody` IR containing declarations, control headers, statements, comments, and opaque units. A separate procedural layout pass owns indentation and blank-line policy. This enables compact and multi-statement single-line bodies without weakening the outer PostgreSQL parser boundary.

`ASSERT` and `RETURN QUERY` are formatter-owned. Trustworthy opaque transaction-control spans are preserved and diagnosed per statement, so supported routine siblings still format under the default unsupported policy; strict policy restores the complete routine/document.

## Go interpreted-string and corpus contract

The Go host layer classifies string expressions structurally. It owns raw literals, interpreted literals, and compile-time concatenations containing only literal operands in declarations, assignments, return values, direct/nested call arguments, `defer`/`go` calls, and composite literal values. Dynamic expressions and non-expression strings remain byte-identical.

Interpreted strings are decoded with the complete Go escape grammar. A multiline formatted value is emitted as a raw string only when no raw-string blocker exists; otherwise it is deterministically escaped. The emitted literal is decoded and compared with the intended runtime value before the complete Go source is reparsed. Auto-detected parse failures remain safe skips; explicit malformed SQL remains fatal.

Real-project confidence has two layers: an offline, checked-in multi-package golden project compiled with `go test ./...`, and an opt-in external runner over immutable release refs. The runner reports discovered expressions, eligible candidates, formatted and unchanged SQL expressions, unsupported expressions, potential false-positive parse skips, dynamic expressions, diagnostics, `gofmt`, idempotence, and project-test results.
