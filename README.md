# semblock

`semblock` is a fast, deterministic PostgreSQL formatter implementing
the **Semantic Block SQL** style.

The repository has completed **Batch 2: formatter-core MVP**. The reusable Rust
formatter facade now implements source-aware result and function-argument
lists, authored logical groups, soft/hard widths, comments, compact/expanded
`CASE`, joins, CTEs, and recursive CTE layout. The project CLI, broader
statement coverage, and Go extraction remain later batches.

The formatter uses the existing `pg_query` crate for the real PostgreSQL
parser, scanner, token ranges, comments, and AST. Project code implements the
Semantic Block layout policy, not another SQL parser.

## Project documents

- [Formatter design](docs/formatter-design.md)
- [Implementation checklist](docs/implementation-checklist.md)
- [Upstream baseline](docs/upstream-baseline.md)
- [Batch 1 backend spike](docs/batch-1-backend-spike.md)
- [Batch 2 formatter-core MVP](docs/batch-2-core-mvp.md)
- [Technical handoff](docs/semantic-block-sql-work-handoff.md)
- [Russian style guide](docs/semantic-block-sql-style-guide-ru.md)
- [Source artifact provenance](docs/source/README.md)
- [Repository working rules](AGENTS.md)

The canonical repository-scoped formatting skill is stored in
`.agent-skills/postgresql-sql-format/`. The `.agents/skills/` and
`.claude/skills/` entries are symlinks to that one copy.

## Planned command surface

```text
semblock fmt .
semblock check .
semblock diff .
semblock fmt migrations/001_init.sql
semblock fmt --stdin --filename query.sql
semblock fmt --language go ./internal/...
```

See the design and checklist for the implementation gates. No syntax is
considered supported until it has a parsing, semantic-safety, idempotence, and
golden fixture.

## Development

Rust 1.88, a C toolchain, and libclang are required to build the pinned
`pg_query` backend.

```text
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```
