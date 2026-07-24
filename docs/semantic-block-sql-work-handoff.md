# Work handoff: Semantic Block SQL formatter

You are starting a new technical implementation project. Treat this document as the source of truth for the initial scope, but verify current upstream APIs, crate versions, licenses, and repository state before coding.

## Goal

Build a fast, lightweight, deterministic PostgreSQL formatter CLI implementing the **Semantic Block SQL** style.

The tool must:

1. format standalone PostgreSQL `.sql` files;
2. recursively process project directories;
3. respect `.gitignore` and a dedicated gitignore-style ignore file;
4. find complete SQL statements embedded in Go string literals using a real Go syntax tree;
5. support file-level, declaration-level, and SQL-block ignore directives;
6. provide `fmt`, `check`, and `diff` workflows suitable for local use and CI;
7. preserve human-authored logical list groups instead of trying to infer domain semantics;
8. use existing PostgreSQL parsing/formatting infrastructure rather than inventing a SQL parser from scratch;
9. keep the formatter backend modular so IDE integrations can call the same canonical engine.

The preferred implementation language is **Rust**.

## Durable source materials

The following artifacts were prepared in the previous discussion and should be added to the repository in Batch 0:

- `postgresql-sql-format.zip`
  - portable Codex/Claude agent skill;
  - contains `SKILL.md`, detailed style references, examples, checklist, and eval fixtures.
- `semantic-block-sql-style-guide-ru.md`
  - Russian wiki article for developers;
  - human-oriented explanation with Bad/Good examples.

Preserve these as project documentation rather than keeping them only in chat context.

Suggested repository locations:

```text
docs/semantic-block-sql-style-guide-ru.md
docs/formatter-design.md
.agents/skills/postgresql-sql-format/
.claude/skills/postgresql-sql-format/
```

Avoid maintaining two divergent skill copies. Prefer one canonical skill directory and generate/copy/symlink the client-specific installation layout as appropriate for the target environments.

---

# 1. Style definition

The style is called:

- general name: **Semantic Block SQL**;
- PostgreSQL profile: **PostgreSQL Semantic Block Style**.

The core principle:

> Keep a construct compact while it is easy to understand. Expand it at syntax-tree and logical-group boundaries when length or complexity obscures the structure.

The formatter must display the syntax tree without producing indentation staircases.

## 1.1 Casing

Use uppercase for:

- SQL keywords;
- keyword-like SQL constructs;
- `NULL`;
- `TRUE`;
- `FALSE`.

Examples:

```sql
SELECT
FROM
WHERE
CASE
WHEN
THEN
ELSE
END
COALESCE
NULLIF
FILTER
OVER
ARRAY
NULL
TRUE
FALSE
```

Use lowercase for:

- function names;
- type names.

Examples:

```sql
count(*)
sum(value)
now()
jsonb_build_object(...)
bigint
uuid
timestamptz
```

Preserve exactly:

- quoted identifiers;
- string contents;
- comments;
- dollar-quoted contents unless the body is explicitly parsed as SQL or PL/pgSQL.

For PostgreSQL, prefer `!=` over `<>`.

## 1.2 Indentation and whitespace

- Four spaces per real syntactic nesting level.
- Never use tabs.
- Trailing commas, never leading commas.
- One space after commas.
- Spaces around binary operators.
- No spaces around `::`.
- No space between a function name and `(`.
- Statement semicolon belongs at the end of the final clause, not on a separate line.

Examples:

```sql
score >= 0.75
parent.depth + 1
$1::uuid
count(*)
SELECT id, kp_id, imdb_id;
```

## 1.3 Soft and hard line widths

The formatter needs two limits:

```toml
soft_line_width = 120
hard_line_width = 160
```

Meaning:

- the soft limit allows wrapping but does not force it;
- the formatter may preserve a cohesive human-authored group beyond the soft limit;
- no emitted line may exceed the hard limit when a safe syntax boundary exists;
- strings, quoted identifiers, comments, URLs, and other indivisible tokens may exceed the hard limit;
- never split inside a token.

Both values must be configurable.

## 1.4 Compact versus expanded layout

Keep clauses inline while they remain short and obvious:

```sql
SELECT id, title
FROM public.items
WHERE deleted_at IS NULL
ORDER BY created_at DESC;
```

Expand when:

- there are many arguments;
- the line exceeds a preferred width;
- an argument contains nested SQL;
- mixed `AND` and `OR` logic appears;
- complex parentheses obscure precedence;
- the author already separated logical groups;
- one argument is independently complex.

```sql
WHERE
    item.deleted_at IS NULL
    AND (
        item.title_rus IS NOT NULL
        OR item.title_orig IS NOT NULL
    )
```

## 1.5 Preserve logical groups

The formatter must **not attempt to infer domain-level semantic groups**.

Given:

```sql
SELECT
    item.id, item.kp_id, item.imdb_id,
    item.title_rus, item.title_orig,
    item.created_at, item.updated_at
FROM public.items item;
```

preserve these three authored groups as long as each line stays within the hard limit.

Rules:

1. Existing line boundaries inside list-like clauses are group hints.
2. Existing blank lines are hard logical-group boundaries.
3. Comments are hard logical-group boundaries.
4. Do not merge across a blank line or comment.
5. Do not split an existing group only because it crosses the soft limit.
6. Split a group at safe syntax boundaries when it crosses the hard limit.
7. When formatting completely unformatted one-line SQL, create deterministic groups using syntax only:
   - simple arguments may be packed greedily up to the soft limit;
   - complex arguments receive their own line;
   - do not pretend to infer business meaning.

This is intentionally more preservation-oriented than `gofmt`.

## 1.6 Avoid indentation storms

Connector keywords do not create an empty indentation level by themselves.

### Expanded JOIN

Canonical:

```sql
LEFT JOIN match_new.source_links link ON
    link.kp_id = item.kp_id
    AND link.status = 'approved'
    AND (
        link.model_version = current_model.version
        OR link.match_method = 'manual'
    )
```

Do not emit:

```sql
LEFT JOIN match_new.source_links link
    ON
        link.kp_id = item.kp_id
        AND link.status = 'approved'
```

A simple join stays inline:

```sql
JOIN public.items item ON item.id = source.item_id
```

### MERGE

Canonical:

```sql
MERGE INTO public.items target
USING staging.items source ON target.id = source.id

WHEN MATCHED AND source.deleted_at IS NOT NULL THEN DELETE

WHEN MATCHED THEN UPDATE SET
    title = source.title,
    updated_at = source.updated_at

WHEN NOT MATCHED THEN INSERT (id, title, updated_at)
    VALUES (source.id, source.title, source.updated_at);
```

Do not create:

```sql
WHEN MATCHED THEN
    UPDATE SET
        title = source.title
```

The action stays attached to `THEN`; only the action contents are indented.

For a complex `USING ... ON` condition:

```sql
USING staging.items source ON
    target.id = source.id
    AND target.tenant_id = source.tenant_id
    AND target.deleted_at IS NULL
```

## 1.7 Boolean expressions

Continuation operators begin lines:

```sql
WHERE
    first_condition
    AND second_condition
    AND (
        third_condition
        OR fourth_condition
    )
```

Mixed `AND`/`OR` groups must make precedence visually obvious.

Preserve explicit precedence-significant parentheses.

## 1.8 CASE

Prefer compact branches:

```sql
CASE
    WHEN item.id IS NULL THEN 0
    WHEN item.deleted_at IS NOT NULL THEN -1
    ELSE first_expression + second_expression
END
```

A short complete `CASE` may stay inline:

```sql
status = CASE WHEN source.approved THEN 'approved' ELSE 'rejected' END
```

Only expand independently complex conditions or results:

```sql
CASE
    WHEN
        item.status = 'active'
        AND item.deleted_at IS NULL
        AND item.published_at <= now()
    THEN calculate_score(item.id)
    ELSE 0
END
```

## 1.9 Blank lines

Blank lines may separate logical blocks:

- set-operation branches;
- independent CTEs;
- unrelated join groups;
- large result groups;
- `MERGE` branches;
- DDL columns and table constraints;
- PL/pgSQL algorithm stages.

Do not insert blank lines mechanically between every clause.

## 1.10 Local alignment

Alignment of `AS`, `=`, types, or constraints is optional human polish.

Examples:

```sql
SELECT
    count(*)          AS sessions,
    sum(watched_secs) AS watched_secs
```

```sql
SET
    title_rus  = source.title_rus,
    title_orig = source.title_orig,
    updated_at = now()
```

The formatter does not need to force alignment.

If implemented, alignment must be local and must not:

- create large whitespace gaps;
- cross blank-line groups;
- push lines over limits;
- destabilize diffs unnecessarily.

Correct indentation and grouping are required; visual alignment is not.

## 1.11 Statement-specific conventions

### CTE

```sql
WITH totals AS (
    SELECT item_id, count(*) AS sessions
    FROM stats.sessions
    GROUP BY item_id
),
ranked AS (
    SELECT
        item_id,
        row_number() OVER (ORDER BY sessions DESC) AS position
    FROM totals
)
SELECT item_id, position
FROM ranked;
```

### Set operations

```sql
SELECT id
FROM active_items

UNION ALL

SELECT id
FROM archived_items

EXCEPT

SELECT id
FROM blocked_items;
```

### VALUES

Short rows may stay inline; complex rows may expand independently:

```sql
VALUES
    (
        'ml_v1',
        'classifier',
        jsonb_build_object(
            'threshold', 0.75,
            'features', jsonb_build_array('title', 'year')
        ),
        TRUE,
        now()
    ),
    ('manual', 'human', jsonb_build_object(), TRUE, now());
```

### ON CONFLICT

```sql
ON CONFLICT (kp_id) WHERE kp_id IS NOT NULL
DO UPDATE
SET
    imdb_id = EXCLUDED.imdb_id,
    title_rus = EXCLUDED.title_rus,
    updated_at = now()
WHERE items.deleted_at IS NULL
```

The conflict-target `WHERE` and update-action `WHERE` must remain visually distinguishable.

### UPDATE

```sql
UPDATE public.items item
SET
    title_rus = source.title_rus,
    title_orig = source.title_orig,
    updated_at = now()
FROM staging.items source
WHERE
    source.id = item.id
    AND source.is_valid = TRUE
RETURNING item.id;
```

### DELETE

```sql
DELETE FROM public.items item
USING staging.deleted_items source
WHERE
    source.id = item.id
    AND item.deleted_at IS NOT NULL
RETURNING item.id;
```

### CREATE TABLE

```sql
CREATE TABLE stats.daily (
    item_id bigint NOT NULL,
    day date NOT NULL,
    watch_count bigint NOT NULL DEFAULT 0,

    CONSTRAINT daily_pk PRIMARY KEY (item_id, day),
    CONSTRAINT daily_count_chk CHECK (watch_count >= 0)
);
```

### CREATE INDEX

Simple form stays on one line:

```sql
CREATE INDEX users_reg_date_idx ON users (reg_date);
CREATE INDEX users_reg_date_idx ON users (reg_date) WHERE deleted_at IS NULL;
```

Complex form expands:

```sql
CREATE INDEX item_activity_idx
    ON stats.item_activity (created_at DESC, item_id)
    INCLUDE (watch_count, rating_count)
    WHERE
        watch_count > 0
        OR rating_count > 0;
```

### ALTER TABLE

```sql
ALTER TABLE public.items
    ADD COLUMN projection_status text NOT NULL DEFAULT 'pending',
    ADD COLUMN projection_error jsonb,

    ADD CONSTRAINT projection_status_chk
        CHECK (projection_status IN ('pending', 'done', 'failed'));
```

### PL/pgSQL

```sql
DO $$
    DECLARE
        affected_rows bigint;
    BEGIN
        UPDATE public.items
        SET updated_at = now()
        WHERE deleted_at IS NULL;

        GET DIAGNOSTICS affected_rows = ROW_COUNT;

        IF affected_rows > 0 THEN
            RAISE NOTICE 'updated: %', affected_rows;
        END IF;
    END
$$;
```

## 1.12 Semantic safety

Formatting mode may change:

- whitespace;
- indentation;
- line breaks;
- keyword casing;
- safe PostgreSQL lexical preference `<>` → `!=`;
- comment placement only when attachment to the same syntax node is preserved.

Formatting mode must not:

- add casts;
- reorder columns, predicates, joins, CTEs, or statements;
- rename aliases;
- add or remove predicates;
- optimize queries;
- convert subqueries to joins;
- change literal contents;
- remove potentially meaningful parentheses;
- change SQL semantics.

Any semantic or performance recommendation belongs in a separate review mode, not formatter output.

---

# 2. Technical architecture

## 2.1 Preferred backend

Do not implement a PostgreSQL parser from scratch.

First investigate the current versions and architecture of:

- `libpgfmt`;
- `pgfmt`;
- `tree-sitter-postgres`;
- `tree-sitter-go`.

The preferred route is to extend or fork `libpgfmt` with:

```rust
Style::SemanticBlock
```

If upstream architecture makes this impractical, document the concrete limitations before selecting another parser/formatter foundation.

The implementation must remain PostgreSQL-specific and support modern constructs such as:

- CTE and recursive CTE;
- `INSERT ... ON CONFLICT`;
- `UPDATE ... FROM`;
- `DELETE ... USING`;
- `MERGE`;
- `RETURNING`;
- partial indexes;
- window expressions;
- `FILTER`;
- PostgreSQL casts;
- arrays and JSON expressions;
- PL/pgSQL blocks.

## 2.2 Suggested Rust stack

Verify the latest suitable crates, but the expected stack is:

```text
libpgfmt / pgfmt        PostgreSQL formatter foundation
tree-sitter-postgres   PostgreSQL and PL/pgSQL syntax trees
tree-sitter-go         Go concrete syntax tree and exact source ranges
ignore                 recursive traversal and gitignore semantics
clap                   CLI
serde + toml           configuration
similar or equivalent  unified diff output
tempfile               atomic writes
rayon                   optional parallel processing
```

Do not add dependencies merely for convenience if the standard library or an existing selected dependency already covers the requirement.

## 2.3 Modules

Suggested boundaries:

```text
src/
├── main.rs
├── cli.rs
├── config.rs
├── discover.rs
├── directives.rs
├── formatter/
│   ├── mod.rs
│   ├── semantic_block.rs
│   └── validation.rs
├── host/
│   ├── mod.rs
│   └── go.rs
├── rewrite.rs
├── diff.rs
└── diagnostics.rs
```

Key rule: project discovery and host-language extraction must not know formatter internals.

The formatter API should conceptually be:

```rust
format_sql(source: &str, options: &FormatOptions) -> Result<FormattedSql>
```

The same API should serve:

- CLI;
- future IDEA integration;
- future VS Code integration;
- stdin formatting;
- embedded Go snippets.

---

# 3. CLI contract

Tentative binary name:

```text
semblock
```

Commands:

```bash
semblock fmt .
semblock check .
semblock diff .
semblock fmt migrations/001_init.sql
semblock fmt --stdin --filename query.sql
semblock fmt --language go ./internal/...
```

Required behavior:

## `fmt`

- format files in place;
- write atomically;
- do not partially modify a file if one embedded SQL fragment fails;
- print changed files concisely.

## `check`

- never modify files;
- exit non-zero if formatting changes are required;
- suitable for CI.

## `diff`

- never modify files;
- print unified diffs;
- exit non-zero when differences exist.

Suggested exit codes:

```text
0 = success / already formatted
1 = formatting differences found in check or diff mode
2 = invalid arguments or configuration
3 = SQL or host-language parse failure
4 = filesystem or rewrite failure
```

The exact codes may be adjusted, but they must be documented and stable.

Required global options:

```text
--config <path>
--stdin
--filename <name>
--language <auto|sql|go>
--jobs <n>
--verbose
--quiet
```

Avoid an excessively large v1 CLI.

---

# 4. Configuration and ignore handling

Suggested `semblock.toml`:

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
interpreted_strings = true
```

## Ignore files

Use gitignore-compatible matching.

Automatically respect `.gitignore` by default.

Support a dedicated file:

```text
.semblockignore
```

Example:

```gitignore
vendor/
third_party/
**/generated.sql
migrations/legacy/**
internal/testdata/broken_sql/**
```

Support nested ignore files only if the selected traversal library provides clear and predictable semantics. Document precedence.

---

# 5. Go SQL extraction

Use a real Go syntax tree. Do not scan Go source using regex alone.

The parser must locate string literal nodes and exact byte ranges.

Regex or cheap lexical checks are allowed only after a string literal has already been identified structurally.

## 5.1 Candidate detection

Inspect:

- raw backtick string literals;
- interpreted string literals when they decode to a complete SQL statement;
- literals in `const` declarations;
- literals in `var` declarations;
- literals assigned in short or regular assignments;
- literals passed directly to known SQL APIs may be added later, but are not required for MVP.

A candidate should:

1. not be ignored by a directive;
2. look like SQL using cheap prefix checks;
3. parse as one or more complete PostgreSQL statements.

Possible SQL-prefix check:

```regex
(?is)^\s*(WITH|SELECT|INSERT|UPDATE|DELETE|MERGE|CREATE|ALTER|DROP|DO|CALL|GRANT|REVOKE|TRUNCATE|COMMENT)\b
```

The PostgreSQL parser is the authority. Regex is only a fast rejection filter.

Do not format incomplete fragments such as:

```go
const whereClause = `WHERE deleted_at IS NULL`
```

unless explicit fragment formatting is designed later.

## 5.2 String preservation

For raw string literals:

- preserve the backtick delimiters;
- format only the content;
- preserve the host-file indentation correctly.

For interpreted literals:

- decode safely;
- format;
- re-encode without changing runtime contents;
- if safe round-tripping is not guaranteed, skip with a diagnostic in MVP rather than risk a semantic change.

It is acceptable for MVP to support raw literals first and add interpreted strings after the core is proven, but this scope decision must be explicit and tested.

## 5.3 Directives

Support file-level, declaration-level, and SQL-block suppression.

### File-level Go ignore

```go
// semblock:file-ignore
package legacy
```

The directive must appear in the leading file comments before the package declaration or in another clearly documented location.

### Declaration-level ignore

```go
// semblock:ignore
const legacyQuery = `
    select vendor_specific_magic(...)
`
```

The comment must be attached to the declaration in the Go syntax tree.

### Explicit SQL marker

```go
// semblock:sql
const query = `
    SELECT ...
`
```

Also recognize JetBrains injection comments:

```go
// language=SQL
const query = `
    SELECT ...
`
```

Explicit markers may bypass the cheap SQL-prefix heuristic, but the SQL must still parse completely.

### SQL file-level ignore

```sql
-- semblock:file-ignore
```

### SQL block ignore

```sql
-- semblock:off
SELECT vendor_specific_magic(...);
-- semblock:on
```

The exact behavior for nested or unmatched directives must be documented. Prefer a clear diagnostic over guessing.

---

# 6. Rewrite safety

For standalone SQL:

1. parse original source;
2. format it;
3. parse formatted source;
4. ensure no new syntax errors;
5. format the result again;
6. assert idempotence;
7. write atomically.

For Go files:

1. parse the complete original Go file;
2. find all eligible SQL literal spans;
3. format each snippet independently;
4. if any required snippet fails, do not modify the file;
5. apply replacements from the end of the file backward;
6. parse the resulting Go file again;
7. optionally verify each literal decodes to the same intended SQL semantics;
8. write atomically.

Never leave a partially rewritten file.

Preserve original newline convention if practical.

---

# 7. Testing strategy

Use TDD for formatter rules.

## 7.1 Golden tests

Convert the agent skill examples and wiki examples into golden fixtures.

At minimum cover:

1. compact `SELECT`;
2. expanded result list;
3. complex mixed `AND`/`OR`;
4. compact and expanded `JOIN ... ON`;
5. compact and expanded `CASE`;
6. multiple CTEs;
7. recursive CTE;
8. `UNION ALL` / `EXCEPT`;
9. simple and complex `VALUES`;
10. `INSERT ... ON CONFLICT DO UPDATE`;
11. `UPDATE ... FROM`;
12. `DELETE ... USING`;
13. compact and complex `MERGE`;
14. lateral joins;
15. grouping, `HAVING`, and windows;
16. `CREATE TABLE`;
17. simple and partial one-line `CREATE INDEX`;
18. complex multiline `CREATE INDEX`;
19. `ALTER TABLE`;
20. PL/pgSQL;
21. comments and directives;
22. dollar-quoted strings;
23. existing logical list groups;
24. hard-limit group splitting;
25. `<>` normalization to `!=`.

## 7.2 Properties

Test:

```text
format(format(sql)) == format(sql)
```

Also test:

- original and formatted SQL both parse;
- formatting does not reorder syntax nodes;
- ignored regions are byte-identical;
- blank-line group boundaries survive;
- lines stay within hard limit where safe breaks exist;
- no indentation storm appears after `JOIN ... ON` or `WHEN ... THEN`.

## 7.3 Go fixtures

Cover:

- raw const SQL;
- raw var SQL;
- assigned raw literal;
- multiple SQL strings in one file;
- non-SQL strings;
- explicit `semblock:sql`;
- declaration-level ignore;
- file-level ignore;
- `language=SQL`;
- malformed SQL candidate;
- incomplete SQL fragment;
- comments containing SQL-looking text;
- safe atomic rollback when one snippet fails;
- host-file parse validation after rewrite.

## 7.4 CLI integration tests

Cover:

- file path;
- recursive directory;
- `.gitignore`;
- `.semblockignore`;
- `fmt`;
- `check`;
- `diff`;
- stdin;
- stable exit codes;
- no partial writes.

---

# 8. Implementation batches

Maintain a durable checklist in the repository and commit after each coherent batch.

## Batch 0 — Persist specification

- create repository;
- add the Russian wiki article;
- add the agent skill;
- add this formatter design;
- create `AGENTS.md` with development rules;
- record upstream licenses and selected versions;
- create an implementation checklist.

No formatter code before this batch is committed.

## Batch 1 — Upstream investigation and spike

- inspect `libpgfmt` architecture;
- run existing tests;
- identify style hooks and limitations;
- implement a minimal `SemanticBlock` proof of concept for:
  - keyword casing;
  - four-space indentation;
  - compact `SELECT`;
  - complex `WHERE`;
  - `JOIN ... ON` without an indentation storm;
- document whether upstream extension, fork, or vendoring is the right route.

## Batch 2 — Core formatter

- implement core list layout;
- soft/hard width handling;
- preservation of authored logical groups;
- `CASE`;
- joins;
- CTEs;
- comments;
- safety validation;
- golden tests.

## Batch 3 — PostgreSQL statement coverage

- `INSERT`;
- `VALUES`;
- `ON CONFLICT`;
- `UPDATE`;
- `DELETE`;
- `MERGE`;
- set operations;
- DDL;
- windows;
- PL/pgSQL.

## Batch 4 — Project CLI

- config;
- traversal;
- `.gitignore`;
- `.semblockignore`;
- stdin;
- `fmt/check/diff`;
- atomic writes;
- diagnostics;
- integration tests.

## Batch 5 — Go extraction

- tree-sitter-go integration;
- string literal extraction;
- SQL candidate detection;
- directives;
- raw literal rewrite;
- Go reparse validation;
- integration tests.

## Batch 6 — Performance and polish

- parallel traversal where useful;
- benchmark large repositories;
- avoid parsing unchanged or ignored files;
- improve diagnostics;
- shell completion if worthwhile;
- release artifacts.

## Batch 7 — IDE adapters

Only after the CLI is stable:

- VS Code extension or task wrapper;
- IntelliJ external tool / file watcher integration;
- editor formatting via stdin;
- do not implement a second formatting engine in an IDE plugin.

---

# 9. Non-goals for MVP

Do not initially implement:

- SQL optimization;
- semantic query rewrites;
- schema-aware formatting;
- database connections;
- query execution;
- formatting partial SQL fragments;
- support for every host language;
- a fully custom PostgreSQL parser;
- domain-semantic inference of result groups;
- mandatory visual alignment;
- a native IDEA formatter engine;
- a native VS Code formatter independent from the CLI.

---

# 10. Acceptance criteria for the first usable release

The release is usable when:

1. `semblock fmt .` formats standalone `.sql` files.
2. `semblock check .` is deterministic and CI-safe.
3. `.gitignore` and `.semblockignore` work.
4. Existing list groups and blank-line groups are preserved.
5. Soft and hard line widths behave as defined.
6. `JOIN ... ON` and `MERGE ... THEN` do not create indentation staircases.
7. Core PostgreSQL DML, CTE, `ON CONFLICT`, `MERGE`, DDL, and PL/pgSQL fixtures pass.
8. Raw Go SQL literals are found via a Go CST, formatted safely, and revalidated as Go.
9. File-level, declaration-level, and SQL block ignore directives work.
10. Formatting is idempotent.
11. The formatter never partially rewrites a file.
12. The Russian wiki article and agent skill are stored in the repository.
13. The repository includes install, configuration, CI, and editor integration documentation.

---

# 11. Working rules for the implementation agent

- Read repository instructions such as `AGENTS.md` before editing.
- Verify upstream repositories, crate APIs, versions, licenses, and activity instead of assuming this handoff is current.
- Use existing libraries and upstream formatter architecture wherever practical.
- Do not invent a new SQL parser.
- Work batch by batch and keep the checklist current.
- Commit or otherwise persist every completed batch.
- Run focused tests during development and the full suite before completion.
- Perform a self-review after each batch:
  - semantic safety;
  - architecture boundaries;
  - idempotence;
  - comment preservation;
  - error handling;
  - unnecessary dependencies;
  - dead code.
- Be explicit when upstream limitations force a design change.
- Prefer partial but tested implementation over broad unverified code.
- Do not claim support for syntax without fixtures.
- Keep CLI behavior stable and documented.

## First action

Start with Batch 0. Add the durable documentation and skill artifacts to the repository, inspect their contents, create a tracked implementation checklist, and only then begin the upstream formatter spike.
