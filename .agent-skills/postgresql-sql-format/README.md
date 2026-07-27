# PostgreSQL SQL Format Agent Skill

Agent Skill for formatting PostgreSQL SQL and PL/pgSQL with the Semantic Block SQL style.

## Layout

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

`SKILL.md` is the concise always-loaded contract. References are read only for complex or ambiguous statements.

## Codex

```bash
mkdir -p .agents/skills
cp -R postgresql-sql-format .agents/skills/postgresql-sql-format
```

## Claude Code

```bash
mkdir -p .claude/skills
cp -R postgresql-sql-format .claude/skills/postgresql-sql-format
```

Repository version: `2.0.0` (derived from the preserved `1.0.0` upload).
