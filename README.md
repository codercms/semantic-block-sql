# semblock

`semblock` is a planned fast, deterministic PostgreSQL formatter implementing
the **Semantic Block SQL** style.

The repository is currently at **Batch 0: durable specification**. Formatter
code is intentionally absent until the specification, upstream baseline, agent
skill, and implementation checklist have been committed.

## Project documents

- [Formatter design](docs/formatter-design.md)
- [Implementation checklist](docs/implementation-checklist.md)
- [Upstream baseline](docs/upstream-baseline.md)
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
