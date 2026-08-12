# Project documentation

This index separates user documentation, current contributor documentation, and historical implementation records.

## User documentation

- [User guide](user-guide.md) — CLI workflows, Git selection, Go source, directives, configuration, diagnostics, and exit codes.
- [PostgreSQL coverage](sql-coverage.md) — fixture-backed statement/expression coverage and known unsupported boundaries.
- [Release builds](release-builds.md) — supported release targets, runtime baselines, and artifact workflow.

## Product and formatting contract

- [Core `fmt` / `check` specification](semantic-block-sql-fmt-check-core-spec.md) — authoritative machine behavior.
- [Semantic Block SQL style guide](semantic-block-sql-style-guide-ru.md) — human-oriented Russian guide.

## Architecture and development

- [Formatter architecture](formatter-architecture.md) — modules, ownership IR, safety model, and execution flow.
- [Formatter design decisions](formatter-design.md) — resolved policy and architecture decisions.
- [PostgreSQL extension guide](formatter-extension-guide.md) — compiler-guided procedure for adding syntax.
- [Implementation checklist](implementation-checklist.md) — current status, gates, and remaining work.
- [Comment trailing-whitespace safety fix](comment-whitespace-safety-fix-plan.md) — active architecture plan and acceptance criteria.
- [Repository instructions](../AGENTS.md) — mandatory working rules for contributors and agents.

## Historical implementation notes

These documents record completed development batches. They are useful for provenance and debugging, but they are not the primary product documentation.

- [Upstream baseline](upstream-baseline.md)
- [Batch 1 backend spike](batch-1-backend-spike.md)
- [Batch 2 formatter-core MVP](batch-2-core-mvp.md)
- [Batch 3 query and MERGE coverage](batch-3-query-merge.md)
- [Runnable CLI and Go MVP](batch-4-5-cli-mvp.md)
- [Values, windows, lateral sources, and DDL](batch-5-values-windows-ddl.md)
- [Relation sources and views](batch-6-sources-views.md)
- [Initial work handoff](semantic-block-sql-work-handoff.md)

## Provenance and licensing

- [Source artifact provenance](source/README.md)
- [Third-party notices](../THIRD_PARTY_NOTICES.md)
- [License](../LICENSE)

## Agent formatting skill

The single canonical skill is stored at:

```text
.agent-skills/postgresql-sql-format/
```

It is guidance for agents and human-assisted formatting. The core machine specification remains authoritative for `semblock fmt` and `semblock check` behavior.
