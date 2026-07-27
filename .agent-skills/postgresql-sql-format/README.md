# PostgreSQL SQL Format Agent Skill

Vendor-neutral agent skill for formatting PostgreSQL SQL and PL/pgSQL with the Semantic Block SQL style.

## Canonical repository location

```text
.agent-skills/postgresql-sql-format/
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

`SKILL.md` is the concise contract. References are loaded only for complex or ambiguous statements.

This directory is the only maintained skill copy in the repository. Do not edit a client-specific copy and then copy it back.

## Codex installation

From the repository root:

```bash
mkdir -p .agents/skills
cp -R .agent-skills/postgresql-sql-format .agents/skills/postgresql-sql-format
```

## Claude Code installation

From the repository root:

```bash
mkdir -p .claude/skills
cp -R .agent-skills/postgresql-sql-format .claude/skills/postgresql-sql-format
```

These installation copies are local client configuration and should not be committed. Keeping one canonical repository copy avoids symlink portability issues and divergent instructions.

Repository skill version: `2.0.0`.
