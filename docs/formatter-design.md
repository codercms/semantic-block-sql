# Formatter design

Status: **Runnable CLI and raw-Go MVP complete**

Last updated: **2026-10-06**

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

- CLI diagnostics render the one-based effective-source `line:column` first and
  retain the diagnostic's half-open UTF-8 byte range as secondary metadata.
  `check` and `diff` use the original input source. Successful `fmt` uses the
  final formatted file, or formatted stdout for `--stdin`; fatal and
  strict-policy `fmt` failures use the unchanged input. The reusable source
  formatter exposes both coordinate spaces: `FormattedSource::diagnostics`
  remains input-relative for compatibility, while
  `FormattedSource::output_diagnostics` is relative to `output`.
- Output-relative diagnostics come from the formatter's already-required
  idempotence pass. The implementation does not track incremental offset
  deltas and does not introduce another parse or formatting pass.
- Go interpreted strings are enabled after a complete decode/format/re-encode
  round trip and runtime-value verification were implemented.
- Go files whose CST comments before the package clause contain both
  `generated` and `DO NOT EDIT`, matched case-insensitively and possibly across
  separate comments, remain byte-identical by default. This preserves Go's
  standard marker while accepting common Swag and Hero variants without
  scanning strings or body comments. `[go].ignore_generated_files = false`
  opts into processing them. Auto-detected non-UTF-8 Go byte strings are
  non-candidates, while explicit SQL markers keep their fail-safe diagnostic
  behavior. This host-language policy is outside the core SQL contract.
- Document-level semicolon discovery remains a byte-offset scanner so its
  ranges agree with PostgreSQL and `SourceRange`. Dollar-quote delimiter
  comparisons are byte-based as well; the scanner must never construct an
  `&str` at an arbitrary byte offset inside UTF-8 routine content.
- Multiline interpreted SQL prefers a raw literal when lossless and otherwise
  uses deterministic interpreted escaping.
- Default configuration is:

  ```toml
  [go]
  enabled = true
  auto_detect = true
  ignore_generated_files = true
  raw_strings = true
  interpreted_strings = true
  multiline_string_style = "prefer_raw"
  ```

- The original ZIP is retained as immutable provenance. The unpacked
  `.agent-skills/postgresql-sql-format/` directory is the active canonical
  skill.

Any future ambiguity is recorded here before implementation.

### Desired coverage recorded by red tests

The explicit requirement to cover unsupported migration syntax and formatter
regressions is implemented with invented fixtures. The original 36 cases and all
22 follow-up cases pass through reviewed AST/ownership/planner contracts. This
includes session settings, aggregate definitions, index attachment, SQL routine
forms/options, procedural adapters, VALUES sources, CTE-backed views, trigger
transition tables, and the subsequent layout/diagnostic reductions. The coverage
report records their fixture-backed scope; PostgreSQL support remains closed.

Existing rejection fixtures describe the current implementation. When a desired
capability is implemented, reconcile its former rejection fixture and coverage
entry with the newly reviewed AST/ownership/planner contract; retain adjacent
unreviewed forms as negative tests. Do not satisfy the red tests with a generic
fallback, an unsupported-diagnostic suppression, or copied production SQL.

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

### Parser-owned alias casing

PostgreSQL scanner keyword categories are not semantic roles. For example,
`no` is grammar in `FOR NO KEY UPDATE` but an alias in
`FROM numbered_offers no`. The formatter therefore retains parser-owned alias
names in the typed capability IR and binds them, in source order, to exact
token indices inside their existing statement, query, CTE, output-list,
window, view, or relation-source owner. Those tokens receive an identifier
role and preserve their authored spelling; the same role-aware token stream is
used for layout widths, rendering, and diagnostics.

This resolves the earlier scanner-only behavior in favor of typed ownership.
Production code has no PostgreSQL keyword exception list and does not infer
aliases with a document-wide scan. A missing, out-of-order, or
contradictory binding is an ownership failure, and the post-format safety gate
also requires the complete bound identifier sequence to retain exact spelling.
The pinned keyword tables are test data only.

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
- change string, quoted identifier, dollar-quoted contents, or comment bytes
  other than terminal Unicode whitespace at a physical comment line end;
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

A source file remains the atomic filesystem-write unit. A top-level parse or
split failure, host reparse failure, project preflight failure, or write failure
leaves the original file unchanged. Once PostgreSQL has established trustworthy
top-level statement spans, the default policy may reconstruct a document from
successfully formatted statements and byte-identical skipped statements. No
statement is ever partially formatted and no file is ever partially written.

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

The explicit project request for JSON object key/value readability supersedes
the generic one-argument-per-line rule and preservation of a break between a
reviewed key and its value. Core section 10.1 now defines that limited exception.
Authored grouping between pairs and all comment/blank boundaries retain priority.
The preference must be carried from AST call capabilities through typed argument
ownership into the existing list planner; it is not a renderer name scan.

Core sections 5.4 and 6 take precedence over compact-query preferences:
authored breaks before typed SELECT and DML clauses remain boundaries even when the
whole query fits on one line. Preserve blank clause boundaries as well. Keeping
one authored boundary does not force otherwise inline sibling clauses to break;
structural complexity and width can still require additional breaks. This
decision belongs to the canonical query planner, using `QueryClauses` and
scanner gap metadata, rather than host adapters or a source-text bypass.
`groups::plan_clause_boundaries` supplies the same rule to query, DML, and DDL
planners. It receives only typed clause locations, never guesses boundaries
from keyword spelling, and preserves blank gaps when a planner requires a break.
Mandatory CREATE TABLE column layout and ALTER action-category separation retain
priority, while authored blank action boundaries remain hard boundaries.
`DEFAULT VALUES` is one typed INSERT source whose boundary is `DEFAULT`, so
planning cannot strand `DEFAULT` on the INSERT header. Authored `DO NOTHING`
boundaries and short inline lists on an authored conflict `SET` clause are retained.
`tests/authored_query_clauses.rs` and `tests/coverage_layout_matrix.rs` cover
COUNT, DML, conflict actions, DDL, partial layouts, suffixes, nested queries,
comments, and unchanged Go/Rust literals.

Within list-like syntax, original non-empty line groups are authored groups
that remain stable while safely breakable within the hard limit. Predicates use
the same rule for a break after `ON` / `WHERE` / `HAVING` and for breaks before
root `AND` / `OR` connectors. Blank lines and comments are hard boundaries.
Line breaks owned only by a nested child expression do not expand its parent.

The formatter:

- preserves an authored list group while it is at or below hard width;
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

The core specification's comment-preservation and no-trailing-whitespace rules
are resolved as follows: characters accepted by Rust `char::is_whitespace()`,
excluding CR and LF terminators, are layout whitespace when they occur at the
end of any physical line inside `--` or `/* ... */` comments. Rendering,
protected-token equivalence, and diagnostics share this exact normalization.
All other comment bytes and the authored line-ending convention remain
protected. Consequently, cleanup of comment-end whitespace is a normal fixable
`spacing.trailing_whitespace` change and never a `format.statement_skipped`
safety failure.

### Hard-width result

After layout, the formatter validates every output line. A remaining
over-hard, breakable line is an error rather than silently violating the
contract. When an indivisible string, quoted identifier, identifier, or comment
necessarily exceeds hard width, formatting succeeds and returns
`FormatWarning::IndivisibleTokenExceedsHardWidth { line, width }`.
An indivisible token excuses the line only when the token cannot fit at its
required indentation, or when an attached comment itself crosses the limit; a
short token merely located after the limit does not make a breakable line
unavoidable. The low-level warning retains the proposed-output line for API
compatibility, while the shared diagnostic aligns the causing output token with
its original token and reports the corresponding source line and byte range.

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

Each core diagnostic carries:

```text
rule_id
severity
message
source_range   # UTF-8 byte range in the source analyzed by that pass
fix_available
```

SQL directive ranges are shifted to document offsets, CRLF normalization is
mapped back to the matching input or output byte offsets, and Go-host
diagnostics are conservatively attributed to the complete owning literal until
exact envelope mapping is implemented. The application-level `FormattedSource`
stores first-pass diagnostics against the original input and second-pass
diagnostics against the final output. Because the second pass is also the
idempotence gate, this dual-coordinate contract adds no parsing work.

The API must serve:

- standalone file formatting;
- stdin;
- Go embedded SQL;
- tests;
- future VS Code and IDEA adapters.

The facade remains pure and performs no filesystem writes. The CLI renders
diagnostics as `path:line:column (bytes start-end): severity[rule_id]: message`.
Successful filesystem `fmt` writes a file atomically before it emits that
file's output-relative diagnostics, so a failed replacement cannot publish
coordinates for bytes that were not installed. `fmt` and `diff` suppress fixed
style errors but still surface warnings.

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
--language <auto|sql|go|rust>
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

Four-space indentation and authored group, blank-line, and comment-boundary
preservation are fixed core behavior. Obsolete configuration keys for those
rules are rejected by strict TOML parsing.

Configuration starts with built-in defaults, then applies the first
`semblock.toml` found from the current directory upward. `--config` replaces
that search with an explicit path. Unknown keys, invalid widths, unsupported dialects, path-like ignore filenames,
and invalid policy values are configuration errors.

### Configurable type-alias resolution

The latest explicit project requirement adds an optional strict
`[format.type_aliases]` table. The empty table preserves all authored type
spellings. Each key selects one closed PostgreSQL built-in alias family and its
value must be another spelling from that family. Normalization is bound to
parser-owned `TypeName` locations (and typed PL/pgSQL declaration nodes), then
reuses parse, protected-range, equivalence, and idempotence gates. Unsupported
statement ranges remain byte-identical.

This resolves the apparent request for `varchar`/`text` conversion by excluding
it: PostgreSQL treats them as distinct types with different length/coercion
behavior. Arbitrary user replacement pairs are likewise excluded because they
cannot satisfy the formatter's schema-free semantic-preservation contract.
Projects opting into unqualified shorthand aliases assume those built-in names
are not shadowed through `search_path`.

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

Top-level transaction control follows the same closed utility path. The
formatter accepts `TransactionStmt` only for `BEGIN` with parser-owned
`ISOLATION LEVEL`, `READ ONLY` / `READ WRITE`, and `[NOT] DEFERRABLE` modes, or
for `COMMIT` whose AST chain flag is false, including explicit `AND NO CHAIN`.
`WORK` and `TRANSACTION` spellings that map to those AST shapes are accepted.
These statements need no transaction-specific
nesting planner: the generic utility binder preserves token order, while a
statement-scoped casing rule handles their unreserved keywords. Leading
comments remain attached to `BEGIN`. `START TRANSACTION`, rollback, savepoint,
prepared-transaction, and chained-commit shapes remain byte-identical with
`syntax.unsupported`; PL/pgSQL transaction control remains opaque as described
below.

`CREATE TYPE ... AS ENUM` remains a validated `CreateEnum` utility, but its
owned `ENUM (...)` value range participates in the shared parenthesized-list
planner. Authored value lines and inline comments therefore force a newline
after `(`, four-space value indentation, and a statement-aligned closing `)`.
Registration requires both the typed utility kind and its owned `ENUM`
parenthesis, so unrelated utility parentheses cannot enter list planning.

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

`ALTER TABLE ... SET (...)` and `RESET (...)` relation options are AST-counted,
bound as inner comma-delimited action lists, and planned independently from the
outer ALTER action. Authored option groups remain intact through the hard limit;
an ungrouped list that requires expansion uses one option per line.

Fatal hard-width errors are produced by statement-local validation, then shifted
into formatted-document line coordinates at every composition boundary. This
applies both to ordinary multi-statement documents and to recursively segmented
`COPY ... FROM STDIN` documents, whose formatted header and protected payload
may precede the failing statement. The ALTER relation-option fixture exposed the
general attribution defect; line-coordinate handling is not specific to ALTER.

Table and column `CHECK` constraints carry AST-validated constraint counts into
token ownership for `CREATE TABLE` and `ALTER TABLE`. The binder proves each
`CHECK (` wrapper inside its owning element or action, then passes only the
inner expression to the ordinary predicate planner with the wrapper's
additional indentation. Boolean precedence, authored parentheses, comments,
and source token order therefore follow the same layout rules as `WHERE`
without treating `CHECK` as a query clause or discovering it globally.

### Shared layout-group decision

`semantic_block/groups.rs` is the single compact-versus-expanded policy for
comma lists and Boolean/expression owners. Owners still define grammar-specific
safe boundaries, but they do not independently reinterpret soft width,
structural complexity, comments, blank lines, or unavoidable overflow.
Authored comma-list groups and predicate root boundaries are passed as
required expansion facts. If an authored predicate is otherwise compact, the
planner preserves only those authored root breaks; if structure or width also
requires expansion, it may add breaks at the remaining safe connectors. Thus
an inline short predicate stays inline while an authored multiline predicate
stays multiline.

### Owned Boolean expressions

The 2026-08-03 Boolean-expression regression requirement supersedes the older
predicate-only planning assumption. Boolean layout is context-independent once
an expression owner has been proven: predicates, SELECT and RETURNING items,
assignment right-hand sides, VALUES items, CASE conditions and results, and
function arguments all derive typed half-open expression ranges from the
already-bound statement/query/list IR.

The shared planner expands mixed or independently complex groups, keeps short
cohesive child groups inline, places owned `AND`/`OR` connectors at continuation
starts, and aligns closing parentheses with their group. Owner metadata keeps
grammar attachment intact: for example `JOIN ... ON`, `SET column = (`, and
`WHEN (` are not split into connector-only tiers. Parenthesized-list analysis
also treats a mixed-Boolean item as complex on the first pass, preventing a
second-pass-only list expansion. Unsupported syntax is still preserved with
`syntax.unsupported`; this change does not broaden AST support.

Short same-precedence predicates remain compact when authored inline and fit
the soft width. A valid authored break after the predicate owner or before a
root connector is preserved. Mixed precedence, nested SQL, attached comments
or blank lines, and width may add further safe breaks. When an expanded
Boolean expression contains `EXISTS` or another nested query, query planning
expands that nested query independently while allowing its own short local
predicate to remain inline. INSERT target-list width is measured from the
owned `INSERT` body start, so leading CTE comments or WITH definitions cannot
force an otherwise compact column list to expand.

## Parser-backed PL/pgSQL routine bodies

`DO` blocks and dollar-quoted PL/pgSQL function/procedure bodies use a dedicated
nested parser boundary. `parse_plpgsql` nodes are adapted into typed capability
categories and scanner-bound source spans, then rendered through a procedural
layout IR independent from authored line boundaries. Compact bodies, declarations,
blocks, SQL statements, assignments, conditionals, loops, `FOREACH`, procedural
`CASE`, dynamic execution, cursor operations, diagnostics, exceptions, `ASSERT`,
and reviewed `RETURN QUERY` forms are covered. Transaction control remains opaque
and follows the default/strict unsupported policy.

Parser-owned `RETURN` expressions and assignment right-hand sides reuse the
canonical SQL expression layout by formatting a synthetic `SELECT` wrapper and
removing only that wrapper. Assignment boundaries come from scanner tokens
inside the typed assignment leaf. The procedural layout retains the resulting
relative indentation inside its body frame, including authored function-argument
groups; formatting may add safe breaks but does not collapse those groups. This
closes the local architecture gap where token-only leaf normalization collapsed
nested queries and other safely breakable expressions onto one over-hard line;
it does not broaden the PL/pgSQL node allowlist.
Typed dynamic `EXECUTE` leaves likewise bind top-level `INTO` and `USING`
clauses from scanner tokens. The command expression, target list, and parameter
expression list reuse the same canonical expression layout, while authored
clause boundaries remain mandatory. Procedural layout supplies the leaf's
remaining soft and hard width after body indentation, so nested control-flow
depth cannot create a line that was only safe at column zero.
Expanded SELECT targets and parenthesized argument lists apply their item
indent as a structural-depth-aware floor; an outer list must not flatten layout
already owned by a nested query.

## SQL-standard routine bodies

`LANGUAGE SQL` functions and procedures with `BEGIN ATOMIC` use a separate
outer-routine boundary from dollar-quoted PL/pgSQL. The reviewed subset owns
exactly one SQL body statement, formats that statement through the canonical
ordinary-SQL pipeline, indents it one level, and validates the complete routine
with PostgreSQL structural equivalence and document idempotence. Function
parameter defaults are parser-owned and preserved; they are not the cause of an
SQL-body diagnostic. The actual routine `LANGUAGE` option is located from the
parser-owned `DefElem.location`; same-spelled parameter names and return-type
identifiers remain identifiers. The outer routine kind (`FUNCTION` or
`PROCEDURE`) is likewise scoped to an AST-proven routine header instead of a
document-wide keyword rule. Multi-statement bodies and unreviewed routine
attributes remain byte-identical with `syntax.unsupported`.

Set operations now use a complete bounded owner rather than an
operator-plus-next-`SELECT` record. Each owner contains all operators, all
branches, and authored branch wrappers inside one CTE body, derived source, or
statement range. Branch cardinality is checked during binding, and planners
consume the owned branches without a second `UNION` scan. Final query suffix
kinds are accepted only when the validated set-operation AST owns them; the
shared lexical clause recognizer merely locates those parser-proven suffixes.
This prevents same-spelled qualified identifiers from truncating a branch,
prevents an operator in a parenthesized CTE from claiming the following CTE,
and gives a `FROM (SELECT ... UNION ... SELECT ...)` source one coherent
indentation owner.

## Statement-granular opaque policy

The formatter classifies every parser-proven top-level statement independently. A supported statement is formatted through the normal validation, ownership, equivalence, protected-token, hard-width, and idempotence gates. A valid but unsupported statement is copied byte-for-byte and receives `syntax.unsupported`. A statement that enters the supported pipeline but cannot pass one of those formatter gates is also copied byte-for-byte and receives `format.statement_skipped`, including its source line in the message. When the failing gate supplies a trusted source range, the diagnostic points to that cause; otherwise it falls back to the complete statement range. Opaque source ranges are tracked separately so a precise diagnostic never exposes style findings from a statement preserved wholesale.

The default `UnsupportedPolicy::Skip` reports both opaque outcomes as warnings and continues with supported siblings. `UnsupportedPolicy::Error` elevates both to errors and returns the complete original document unchanged. A PostgreSQL parse or split failure remains document-fatal because statement boundaries are not trustworthy. This explicit project requirement supersedes the earlier file-wide formatter-safety rule while retaining statement atomicity and atomic filesystem replacement.

`COPY ... FROM STDIN` is split into an AST-validated header plus a protected payload ending at `\.`. Only the header is formatted; payload bytes are never scanned or rewritten.

## Typed PL/pgSQL semantic and layout IR

Routine bodies are no longer formatted from authored lines. `parse_plpgsql` is adapted into typed parser capability categories, while PostgreSQL scanner tokens are bound into a span-bearing `RoutineBody` IR containing declarations, control headers, statements, comments, and opaque units. A separate procedural layout pass owns indentation and blank-line policy. This enables compact and multi-statement single-line bodies without weakening the outer PostgreSQL parser boundary.

`ASSERT`, `RETURN QUERY`, and reviewed COMMIT/ROLLBACK commands are formatter-owned.
Transaction kind and chain metadata are bound before the shared token emitter is
used. Unknown parser nodes preserve the enclosing routine with an unsupported
diagnostic; strict policy restores the complete document.

## Go interpreted-string and corpus contract

The Go host layer classifies string expressions structurally. It owns raw literals, interpreted literals, and compile-time concatenations containing only literal operands in declarations, assignments, return values, direct/nested call arguments, `defer`/`go` calls, and composite literal values. Dynamic expressions and non-expression strings remain byte-identical.

Interpreted strings are decoded with the complete Go escape grammar. A multiline formatted value is emitted as a raw string only when no raw-string blocker exists; otherwise it is deterministically escaped. The emitted literal is decoded and compared with the intended runtime value before the complete Go source is reparsed. Auto-detected parse failures remain safe skips; explicit malformed SQL remains fatal.

Real-project confidence has two layers: an offline, checked-in multi-package golden project compiled with `go test ./...`, and an opt-in external runner over immutable release refs. The runner reports discovered expressions, eligible candidates, formatted and unchanged SQL expressions, unsupported expressions, potential false-positive parse skips, dynamic expressions, diagnostics, `gofmt`, idempotence, and project-test results.


## Rust embedded SQL host contract

The explicit Rust-support requirement adds `.rs` extraction alongside Go, not a
new SQL engine or SQL style policy. Automatic detection and explicit force/ignore
markers mirror Go. Rust ordinary and raw literals in expression positions,
including `const` / `static`, share the canonical PostgreSQL formatter.
Dynamic concatenations, string method receivers, attributes, patterns, ABI and byte/C strings are excluded
structurally. Rust macro token trees are not ordinary function arguments: only
the direct SQL positions of the six documented `sqlx::query*` macros are
reviewed; the typed forms validate their first argument with Syn. Macro expansion,
`format!`, `concat!`, file macros, and renamed/custom macros remain opaque.

Literal decoding uses Syn rather than a handwritten Rust escape grammar.
Physical CRLF is normalized before decoding, as Rust does before tokenization;
escaped carriage returns remain value bytes. SQL receives root indentation,
while authored boundary newlines and closing host indentation are retained.
Existing raw hashes are preserved when safe, multiline ordinary strings prefer
raw output, and the emitted literal is decoded and compared before the whole
Rust source is reparsed. The source dispatcher supplies input/output diagnostic
coordinates and idempotence. Strict unsupported policy restores the whole source
and prevents project writes. Existing four-argument source and discovery APIs
remain available with default Rust settings; explicit Rust settings use the
new `*_with_rust` APIs.

Rust 1.88 / edition 2024 is the formatter's minimum build version, not a claim
that Rust 1.88 is the latest release or a host-source version cap. The pinned
CST grammar controls accepted host syntax; unrecognized syntax fails closed.
No toolchain/MSRV bump is required for this adapter.

The subsequent explicit requirement is SQL coverage parity with Go, using the
existing SQL rather than Rust-specific queries. The shared host regression
matrix, equivalent permanent project goldens and opt-in SQLx compilation fixture
are documented in `docs/host-sql-parity-tests.md`. Comparing the original Go
migration exposed Rust's first-line-only host dedentation: multiline Rust SQL
now removes the same authored host-indent prefix from every body line, keeping
inner indentation and the closing envelope. This resolves the discrepancy in
the host adapter without changing PostgreSQL formatting policy.

Dependency review: `tree-sitter-rust = 0.24.2` is the upstream MIT grammar,
crate source revision `e2bee853694a1d3e0f6ef308fe3674542fec95d7`, released
through the actively maintained tree-sitter Rust project. It reuses the existing
`tree-sitter = 0.26.11` runtime, language ABI, and `cc` dependency; its published
manifest declares edition 2021 without an explicit MSRV. Building and testing
on the pinned Rust 1.88 toolchain verifies our required baseline. Syn 2.0.119
is already in the lockfile, is MIT OR Apache-2.0 licensed, declares Rust 1.71,
and is directly enabled only for `derive` and `parsing` (string and type parsing).
The MIT license texts are retained in third-party notices. No fork, vendored
backend, new parser runtime, or database dependency is introduced.


### Reviewed view CTE and wrapped-query coverage

The explicit desired-support requirement supersedes the former unsupported
boundary for CTE-led view and materialized-view queries. Both now carry typed
CTE ownership into the shared work queue. View AS binds the query's first token,
including a WITH prefix or branch parenthesis; set-operation owners exclude
CREATE headers and check/data suffixes. Adjacent unreviewed expressions remain
byte-identical with `syntax.unsupported`. Synthetic fixtures cover nested CTEs,
comments, alias/storage options, check/data suffixes, and wrapped UNION branches.


### Reviewed migration utility coverage

The desired-support requirement is implemented for ordinary SET values/defaults/
current values, ordinary aggregate definitions, index partition attachments, and
multiline composite fields. The shared utility ownership model carries aggregate
signature/option and composite-field cardinality into bounded list owners. It
reuses the canonical list renderer and semantic/idempotence gates. Star aggregate
signatures have an explicit variant because PostgreSQL represents them with a
missing parameter node. Ordered-set signatures, SET TRANSACTION, and unrelated
index actions retain fixture-backed unsupported boundaries.


### VALUES source support and diagnostic locations

The explicit desired-support requirement includes VALUES derived relations in
SELECT/view and DML relation owners. Their typed capability and wrapper/row
ownership extend the existing VALUES renderer; nested join grouping and each
row's own width are preserved. Unreviewed ORDER BY suffixes remain opaque.
When PostgreSQL provides a first-expression location for such a rejected VALUES
shape, diagnostics point to its bounded VALUES construct instead of the entire
outer statement. Source-relative byte offsets are retained through document/CLI
coordinate translation, including CRLF and UTF-8 prefixes.


### Statement diagnostic anchors after dump comments

The explicit diagnostic-location requirement treats a statement's first SQL
token as the fallback anchor. PostgreSQL RawStmt spans may include attached
leading dump comments; those comments remain inside the statement's immutable
rewrite/opaque span, but unsupported and skipped-statement fallbacks no longer
point at their leading `--` separator. The shared diagnostic builder uses scanner
tokens to locate syntax and preserves existing ranges if scanning is unavailable.
Trusted cause ranges remain exact. Skipped-statement messages adjust their
absolute statement line consistently. File fmt recomputes these locations in
its output pass; check/diff retain input coordinates. Synthetic CLI coverage
includes Unicode, CRLF, preceding layout changes, and repeated fmt calls.


### Authored whitespace after comments

Blank lines between a comment and following SQL are hard authored boundaries,
including dump header comments before DDL. The shared token emitter retains the
next token's authored newline count when terminating a line or block comment.
Layout plans can still supply the following token's indentation without reducing
that gap. Comment attachment and inter-statement spacing remain under their
existing ownership rules. Synthetic fixtures cover dump index headers, line and
block comments, multiple blank lines, and comments within query groups.


### Reviewed SQL-standard multi-statement and RETURN bodies

The explicit remaining-support requirement supersedes the former rejection of
multi-statement SQL-standard bodies. SELECT/DML and RETURN are closed AST-owned
body variants; unknown statement families remain unsupported. RETURN uses the
canonical expression layout and safety gates through an equal-length SELECT
prefix adapter. Inline authored boundaries and atomic body groups are preserved,
with indentation accounted for in the body width budget. Parallel option values
are explicitly reviewed. Nested unsupported failures retain the complete original
routine and report body-relative locations shifted to the enclosing source.

### Reviewed dollar-quoted SQL bodies

The explicit remaining-support requirement supersedes the former non-PL/pgSQL
rejection for LANGUAGE SQL. Declaration metadata is parsed in its owning
statement frame; document-relative AST locations are never applied to slices.
The AS literal is bound through its DefElem location and decoded AST value.
Embedded SQL passes its own structural and protected-token comparison, while
the outer declaration is compared with its original literal restored. No
generic literal exemption is introduced. Multiline token continuation bytes
are never indented. Unreviewed quoting or inner syntax preserves the routine.

### Shared routine header layout

Routine adapters bind declaration signatures and RETURNS TABLE columns against
AST parameter counts, then reuse the canonical parenthesized list planner and
token emitter. Option clauses are located through their DefElem metadata.
The declaration prefix is isolated from its body; header wrapping does not
change body tokens or recase identifiers/types. Original comments, list groups,
defaults, and framing gaps remain subject to the existing safety gates.

### Reviewed migration DDL clause layout

The requested remaining DDL support carries explicit sequence-option kinds and
locations, trigger timing/column/transition-table capabilities, and foreign-key
key counts and action kinds. Token binders verify these capabilities before
producing clause boundaries for the existing planners. This replaces neither
the PostgreSQL parser nor the closed ownership model. Trigger ROW transition
aliases remain unreviewed; OLD/NEW TABLE aliases are separately identifier-owned.

### AST-owned explicit function names

Keyword-tokenized names such as replace must not lose their argument-list
ownership. The completed AST traversal records explicit call locations and
qualified names, distinguishing the pinned parser's operator escape helpers
from authored calls. Binders verify names and argument parentheses, then assign
a function-name source role used by rendering and list planning. A role view is
also available while binding relations, so a call cannot masquerade as a
same-spelled alias. AS-less recordset aliases are verified against their typed
following column-definition capability. Scanner trivia never removes call
ownership; comments remain protected by the existing emission and safety gates.

### Identity sequence-option ownership

Identity constraints carry their AST introduction location and reviewed sequence
options. CREATE TABLE column items and ALTER TABLE actions bind the same typed
option capability within their own spans, preserving generation mode and order.
The sequence-option binder accepts an owned token range and depth rather than
inventing a utility statement for an identity child. Absent and multiline option
forms retain the existing semantic, comment, hard-width, and idempotence gates.

### Nested expression and relation group ownership

SELECT and DML use the same typed join predicates, including ON clauses below
parenthesized relation wrappers. Array constructors carry AST locations and
element counts; their bracket lists are distinct from subscripts. The completed
traversal includes array elements and expression-bearing table/constraint fields
omitted by the backend's convenience walker. Unknown children remain unsupported.
After CASE/predicate parents are planned, array bracket subtrees inherit their
actual parent line indentation, preserving nested child layouts. Function and
recordset list width budgets include validated qualified names/relation headers;
already-expanded child groups do not count as a single compact header line.
Named CHECK prefixes wrap only when their predicate is still compact, keeping
existing expanded predicate layouts stable.

### Parser-owned procedural leaf capabilities

PL/pgSQL static SQL queries, transaction commands, and datatype references are
bound against the pinned parser's JSON nodes. SQL leaves must match parser query
tokens after removal of exactly the parser-owned procedural INTO span. The SQL
child uses the canonical formatter; INTO targets are restored at their original
token boundary, with comments and STRICT preserved. No table-target inference
or keyword fallback is used. Transaction spelling is checked against command
kind and chain metadata. Reference datatype spans retain the parser-recorded
spelling; arithmetic percent operators remain ordinary expressions. Both body
passes repeat capability binding, structural equivalence, and idempotence.
Boolean root connectors exclude contained query owners. In particular, NOT
EXISTS cannot claim an inner WHERE connector and introduce a new authored break
on the second pass. Transaction commands use the shared comment-preserving emitter.

### VALUES suffix ownership

Standalone and derived VALUES carry row counts, ORDER BY item counts, and limit/
offset presence. A shared binder consumes consecutive row groups before binding
query suffix clauses, so suffix expression parentheses cannot claim row ownership.
Lexical capabilities must match the AST record, including repeated sources.
CTE bodies use the existing exhaustive VALUES statement variant rather than a
SELECT spec; nested CTEs and their enclosing SELECT/DML keep separate owners.
Suffix lists and clause boundaries reuse the query list planners, while comments
between rows inherit the row group's indentation. Set-operation VALUES branches
and direct INSERT VALUES suffixes remain explicit fixture-backed boundaries.
Enabling VALUES CTEs exposed a CASE branch whose condition and result each fit
but whose combined WHEN/THEN line did not. Typed CASE result ranges now include
the current planned line prefix in their hard-width budget and may begin a
separate result line. No expression syntax is added, moved, or split.

### External routine declaration ownership

C and internal declarations have explicit language and AS literal cardinality
capabilities. Bound library/symbol tokens remain protected, while signatures,
options, and long AS argument groups reuse shared header planning. Common option
validation is shared with SQL routines; unknown languages remain unsupported.
Header layout precedes normalization, and the header is reparsed before applying
location-owned casing so comment whitespace changes cannot invalidate offsets.
The whole declaration passes structural/protected-token and document idempotence
gates; external literal contents never enter an embedded SQL formatter.

### Multiline token warning coordinates

Width warnings bind tokens intersecting the output line, including multiline
comments and dollar bodies. Reviewed SQL/PL routine AS literals are identified
by their AST metadata and exact body values, including on unchanged second runs.
Inner warnings map exact token identity and occurrence back to source bytes;
optional type aliases do not invalidate surrounding token counts. Unprovable
provenance retains the enclosing token range. Ordinary dollar literals retain
literal ownership, and multiline comment fragments exclude CRLF terminators.

### Independent review requirement resolution

The seven independent-review reproductions are formatter defects within reviewed
syntax, not new grammar requests. Comment attachment and blank boundaries take
precedence over routine statement splitting and procedural target compactness.
Header normalization must never consume locations from a different source frame.
Routine header decisions use the shared group policy; both SQL body spellings
share token-aware assembly/indentation. Unsupported procedural SQL children keep
their diagnostic identity and bounded leaf range rather than becoming routine
ownership failures. Default mode can format the enclosing procedural layout while
preserving the unsupported SQL leaf exactly; strict mode returns the original
complete document. The core specification remains authoritative.

### Routine/result maintainability follow-up

Preservation is an internal typed outcome, independent of user-facing diagnostic
IDs. Canonical formatting retains owned opaque spans through its existing gates;
procedural leaf adapters turn that provenance into `Formatted` or `Preserved`
and return diagnostics separately. A shared internal result module carries text,
diagnostics, warnings and source/output protection across adapters. Declaration
ownership and generic helpers belong to `routine_header`, with one AST-backed
constructor used by SQL, external and procedural routines. These changes clarify
module responsibilities without changing public behavior or the core contract.
