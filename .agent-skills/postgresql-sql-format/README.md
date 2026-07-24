# PostgreSQL SQL Format Agent Skill

A portable Agent Skills package for formatting PostgreSQL SQL and PL/pgSQL without semantic rewriting.

## Package layout

```text
postgresql-sql-format/
├── SKILL.md
├── README.md
├── references/
│   ├── STYLE.md
│   ├── EXAMPLES.md
│   └── CHECKLIST.md
└── evals/
    ├── trigger-evals.json
    └── formatting-cases.md
```

`SKILL.md` contains the always-loaded workflow and core invariants. Detailed rules and examples are in `references/` for progressive disclosure. `evals/` contains optional trigger and formatting tests and is not needed during ordinary use.

## Install for Codex

Repository-scoped:

```bash
mkdir -p .agents/skills
cp -R postgresql-sql-format .agents/skills/postgresql-sql-format
```

User-scoped:

```bash
mkdir -p ~/.agents/skills
cp -R postgresql-sql-format ~/.agents/skills/postgresql-sql-format
```

Invoke explicitly as `$postgresql-sql-format`, or let Codex activate it from its description.

## Install for Claude Code

Repository-scoped:

```bash
mkdir -p .claude/skills
cp -R postgresql-sql-format .claude/skills/postgresql-sql-format
```

User-scoped:

```bash
mkdir -p ~/.claude/skills
cp -R postgresql-sql-format ~/.claude/skills/postgresql-sql-format
```

Invoke explicitly as `/postgresql-sql-format`, or let Claude activate it from its description.

## Share one physical copy between both clients

Store the skill in one canonical location and symlink it into both discovery directories:

```bash
mkdir -p .agent-skills .agents/skills .claude/skills
cp -R postgresql-sql-format .agent-skills/postgresql-sql-format
ln -s ../../.agent-skills/postgresql-sql-format .agents/skills/postgresql-sql-format
ln -s ../../.agent-skills/postgresql-sql-format .claude/skills/postgresql-sql-format
```

Check the relative symlink paths for your repository layout before committing them.

## Suggested prompts

```text
Use the postgresql-sql-format skill to format this query without changing behavior.
```

```text
Format every PostgreSQL statement in this migration using the project SQL style. Do not optimize or rewrite the queries.
```

## Validation

The package follows the open Agent Skills structure: one top-level directory, exactly one `SKILL.md`, required `name` and `description` frontmatter, and optional supporting resources.

When `skills-ref` is available:

```bash
skills-ref validate ./postgresql-sql-format
```

## Style note

Version 1.1.0 uses compact clause introducers: expanded joins use `JOIN ... ON` followed by one continuation indent, and `MERGE` actions stay on the `WHEN ... THEN` line.
