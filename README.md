# semblock

`semblock` is a deterministic, fail-safe PostgreSQL formatter and checker implementing the **Semantic Block SQL** style.

It understands PostgreSQL structure instead of formatting by keyword heuristics, preserves comments and intentional logical groups, supports SQL embedded in Go code, and leaves unsupported syntax untouched rather than guessing.

## Highlights

- **PostgreSQL-aware** — formatting is backed by the PostgreSQL parser.
- **Preserves intent** — authored multiline groups, comments, and semantic block boundaries stay meaningful.
- **Fail-safe** — unsupported or ambiguous statements remain byte-identical by default.
- **SQL + Go** — formats `.sql` files and complete PostgreSQL queries inside Go string expressions.
- **Repository-friendly** — recursive discovery, `.gitignore`, `.semblockignore`, staged files, and `--changed-since`.
- **Deterministic** — successful rewrites pass semantic-equivalence and byte-idempotence safety gates.

## What does it look like?

<!-- Transparent intrinsic-width spacers keep GitHub's auto-layout from sizing Before/After columns from code-line length. -->

### Queries and joins

<table width="100%">
<tr>
<th width="50%">Before</th>
<th width="50%">After</th>
</tr>
<tr>
<td width="50%" valign="top">
<img src="docs/assets/readme-column-spacer.png" width="480" height="1" alt="">

<pre><code class="language-sql">select * from a join b on
    a.id=b.a_id
    and a.tenant_id=b.tenant_id;</code></pre>

</td>
<td width="50%" valign="top">
<img src="docs/assets/readme-column-spacer.png" width="480" height="1" alt="">

<pre><code class="language-sql">SELECT *
FROM a
JOIN b ON
    a.id = b.a_id
    AND a.tenant_id = b.tenant_id;</code></pre>

</td>
</tr>
</table>

Authored grouping is intentional: if you split a predicate into logical branches, semblock keeps that structure instead of collapsing it merely because the result would fit on one line.

### Semantic blocks

<table width="100%">
<tr>
<th width="50%">Before</th>
<th width="50%">After</th>
</tr>
<tr>
<td width="50%" valign="top">
<img src="docs/assets/readme-column-spacer.png" width="480" height="1" alt="">

<pre><code class="language-sql">with combined_ids as(
    select 1 as id union all select 2
)
select id from combined_ids;</code></pre>

</td>
<td width="50%" valign="top">
<img src="docs/assets/readme-column-spacer.png" width="480" height="1" alt="">

<pre><code class="language-sql">WITH combined_ids AS (
    SELECT 1 AS id

    UNION ALL

    SELECT 2
)
SELECT id
FROM combined_ids;</code></pre>

</td>
</tr>
</table>

The formatter uses indentation, blank lines, and clause boundaries to make query structure visible rather than merely normalizing whitespace.

### PostgreSQL DML

<table width="100%">
<tr>
<th width="50%">Before</th>
<th width="50%">After</th>
</tr>
<tr>
<td width="50%" valign="top">
<img src="docs/assets/readme-column-spacer.png" width="480" height="1" alt="">

<pre><code class="language-sql">update target set value=source.value
from source
natural left outer join tenant
where target.id=source.id;</code></pre>

</td>
<td width="50%" valign="top">
<img src="docs/assets/readme-column-spacer.png" width="480" height="1" alt="">

<pre><code class="language-sql">UPDATE target
SET value = source.value
FROM source
NATURAL LEFT OUTER JOIN tenant
WHERE target.id = source.id;</code></pre>

</td>
</tr>
</table>

PostgreSQL-specific DML such as `UPDATE ... FROM`, `DELETE ... USING`, and joined relation sources use the same structural layout rules.

## Install

Prebuilt archives are published on [GitHub Releases](https://github.com/codercms/semantic-block-sql/releases).

| Platform | Target |
| --- | --- |
| Windows x64 | `x86_64-pc-windows-msvc` |
| Linux x64 | `x86_64-unknown-linux-gnu` |
| macOS Intel | `x86_64-apple-darwin` |
| macOS Apple Silicon | `aarch64-apple-darwin` |

Download the archive for your platform, verify it against `SHA256SUMS`, extract it, and place `semblock` or `semblock.exe` on `PATH`.

```bash
semblock --version
semblock --help
```

See [release build details](docs/release-builds.md) for runtime baselines and packaging notes.

## Quick start

Format a project:

```bash
semblock fmt .
```

Check formatting without modifying files:

```bash
semblock check .
```

Show the diff:

```bash
semblock diff .
```

Create a project configuration:

```bash
semblock init
```

A typical CI check is simply:

```bash
semblock check .
```

`check` and `diff` exit with code `1` when formatting changes are required.

### Git workflows

Work only with staged files:

```bash
semblock check --staged
semblock diff --staged
semblock fmt --staged
```

Check live files changed relative to the merge base with a reference:

```bash
semblock check --changed-since origin/main
```

Normal project discovery respects `.gitignore` and nested `.semblockignore` files.

For stdin, staged-file semantics, exit codes, directives, configuration, and other CLI details, see the [user guide](docs/user-guide.md).

## PostgreSQL coverage

`semblock` has fixture-backed structural support for the PostgreSQL syntax commonly found in application code and migrations, including:

- `SELECT`, CTEs, set operations, subqueries, windows, grouping, ordering, pagination, and row locking;
- `INSERT`, `UPDATE`, `DELETE`, `MERGE`, `RETURNING`, and `ON CONFLICT`;
- common and advanced relation sources including joins, `LATERAL`, derived tables, functions, `ROWS FROM`, and `TABLESAMPLE`;
- tables and partitions, indexes, views, materialized views, types, domains, sequences, triggers, policies, and reviewed `ALTER` forms;
- migration and operational statements such as `COPY`, `CALL`, `EXPLAIN`, `VACUUM`, `ANALYZE`, `REFRESH MATERIALIZED VIEW`, `GRANT`, `REVOKE`, and `COMMENT ON`;
- PostgreSQL operators and expressions including JSON/JSONB, JSONPath, `hstore`, arrays, ranges, full-text search, regex, and network operators;
- substantial PL/pgSQL support including declarations, conditionals, loops, exception handlers, dynamic `EXECUTE`, cursors, and `RETURN QUERY`.

Some valid PostgreSQL syntax is intentionally outside the reviewed capability set. See [PostgreSQL coverage](docs/sql-coverage.md) for the detailed boundary and known unsupported areas.

## Fail-safe by default

Valid PostgreSQL does not automatically mean semblock is allowed to rewrite it.

If a statement uses syntax outside the reviewed formatter capability set, semblock preserves it **byte-for-byte** and reports `syntax.unsupported`.

If parsing succeeds but formatter ownership or a safety check cannot be proven, semblock likewise preserves the statement and reports `format.statement_skipped`.

Supported sibling statements continue formatting normally. Use strict mode when unsupported syntax should fail CI:

```bash
semblock check --strict-unsupported .
```

The guiding rule is simple: **unchanged SQL is better than a plausible but unsafe rewrite.**

## Go support

Go files are discovered alongside SQL files by default. Semblock understands complete SQL values in raw and interpreted strings across declarations, assignments, returns, calls, composite values, and compile-time string concatenations.

For example:

<table width="100%">
<tr>
<th width="50%">Before</th>
<th width="50%">After</th>
</tr>
<tr>
<td width="50%" valign="top">
<img src="docs/assets/readme-column-spacer.png" width="480" height="1" alt="">

<pre><code class="language-go">const query = `select id,name
from users
where active=true;`</code></pre>

</td>
<td width="50%" valign="top">
<img src="docs/assets/readme-column-spacer.png" width="480" height="1" alt="">

<pre><code class="language-go">const query = `SELECT id, name FROM users WHERE active = TRUE;`</code></pre>

</td>
</tr>
</table>

Runtime expressions and non-SQL strings remain untouched. Rewritten Go files must reparse successfully, and the decoded SQL value is verified after replacement.

Go files whose leading comments identify generated code and say `DO NOT EDIT` are ignored by default. This includes the standard Go marker and common generator variants. Set `ignore_generated_files = false` under `[go]` to opt in to processing them.

```bash
semblock fmt --language go ./internal/...
```

See the [user guide](docs/user-guide.md#go-source) for supported Go expression shapes and string-rewrite behavior.

## Configuration

`semblock` searches upward for `semblock.toml`. Generate the defaults with:

```bash
semblock init
```

Common settings:

```toml
[format]
unsupported_policy = "skip"

[format.type_aliases]
integer = "int"
character_varying = "varchar"
timestamp_with_time_zone = "timestamptz"

[layout]
soft_line_width = 120
hard_line_width = 160

[go]
enabled = true
ignore_generated_files = true
multiline_string_style = "prefer_raw"
```

Indentation is always four spaces. Authored list groups, blank lines, and comment boundaries are structural and are not configurable.

Inspect the effective configuration with:

```bash
semblock config path
semblock config show
```

Type-alias preferences are optional and preserve authored spelling when omitted.
See the [full enabled example](examples/semblock-all-type-aliases.toml) and the
[user guide](docs/user-guide.md#type-alias-preferences) for the validated families.
`varchar` and `text` are distinct PostgreSQL types and are never interchanged.

The full configuration and directive reference is in the [user guide](docs/user-guide.md).

## Documentation

- [User guide](docs/user-guide.md) — CLI workflows, Go source, configuration, directives, diagnostics, and exit codes.
- [PostgreSQL coverage](docs/sql-coverage.md) — supported statement families, expressions, PL/pgSQL, and known capability boundaries.
- [Core `fmt` / `check` specification](docs/semantic-block-sql-fmt-check-core-spec.md) — authoritative machine behavior.
- [Semantic Block SQL style guide](docs/semantic-block-sql-style-guide-ru.md) — formatting rules and rationale.
- [Project documentation index](docs/README.md) — architecture, extension guides, release builds, and historical notes.

## Build and development

Building from source requires Rust 1.88, a C/C++ toolchain, CMake, Clang/`libclang`, and Go when running the complete test suite.

```bash
git clone https://github.com/codercms/semantic-block-sql.git
cd semantic-block-sql
cargo build --locked --release
```

Before changing formatter behavior, read [AGENTS.md](AGENTS.md).

Run the complete local gate:

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo doc --locked --no-deps
git diff --check
```

Detailed architecture and extension guidance is indexed in [docs/README.md](docs/README.md).

## License

MIT. See [LICENSE](LICENSE) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
