# Source artifact provenance

These are the original artifacts supplied for Batch 0. The normalized Markdown
copies in `docs/` and the active skill in `.agent-skills/` are derived from
them.

Snapshot date: **2026-07-25**

| Original upload | Repository copy | SHA-256 |
| --- | --- | --- |
| `semantic-block-sql-work-handoff(1).md` | `docs/semantic-block-sql-work-handoff.md` | `aff8799bc56985cbd3092199a0a300f494b1e624edf7cec17068d8291770f7fc` |
| `semantic-block-sql-style-guide-ru(2).md` | `docs/semantic-block-sql-style-guide-ru.md` | `88aaa1e01cf9a2dfc6d39b089ae4993cbf657440992cbd7c757c1e111501b881` |
| `postgresql-sql-format(2).zip` | `docs/source/postgresql-sql-format.zip` | `53226791417c88a11af8caf791001275f7395a421274e1c7d631b3de9ee03e36` |

The two Markdown copies and the ZIP are byte-identical to the uploads.

The ZIP was unpacked into the canonical directory:

```text
.agent-skills/postgresql-sql-format/
```

Client discovery paths do not contain independent copies:

```text
.agents/skills/postgresql-sql-format -> ../../.agent-skills/postgresql-sql-format
.claude/skills/postgresql-sql-format -> ../../.agent-skills/postgresql-sql-format
```

The archive is provenance only. Edit the canonical unpacked skill if a future
project decision changes it, update its version, and document why. Do not
silently regenerate the original archive.
