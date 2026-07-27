# Source artifact provenance

These are the original artifacts supplied across the project requirements. The
normalized Markdown copies in `docs/` and the active skill in `.agent-skills/`
are derived from them.

Snapshot dates: **2026-07-25** and **2026-07-26**

| Original upload | Repository copy | SHA-256 |
| --- | --- | --- |
| `semantic-block-sql-work-handoff(1).md` | `docs/semantic-block-sql-work-handoff.md` | `aff8799bc56985cbd3092199a0a300f494b1e624edf7cec17068d8291770f7fc` |
| `semantic-block-sql-style-guide-ru(2).md` | `docs/semantic-block-sql-style-guide-ru.md` | `88aaa1e01cf9a2dfc6d39b089ae4993cbf657440992cbd7c757c1e111501b881` |
| `postgresql-sql-format(2).zip` | `docs/source/postgresql-sql-format.zip` | `53226791417c88a11af8caf791001275f7395a421274e1c7d631b3de9ee03e36` |
| `semantic-block-sql-fmt-check-core-spec.md` | `docs/source/semantic-block-sql-fmt-check-core-spec.md` | `24454e01ea4fb75a375a02239eb11c26c1d49586758a6f96b85805db446d24ef` |
| `postgresql-sql-format-1.0.0.zip` | `docs/source/postgresql-sql-format-1.0.0.zip` | `bb40d54ba1fbcf679ad9ac348bcc444d7cf7db2764f0ddf396cad0efb979be6a` |

The repository copies listed above are byte-identical to their uploads. The
2026-07-26 core specification is also copied verbatim to
`docs/semantic-block-sql-fmt-check-core-spec.md` as the active core contract.

The original skill archive was unpacked and subsequently revised in the single
canonical directory:

```text
.agent-skills/postgresql-sql-format/
```

The repository intentionally does not contain separate `.agents/skills/` or
`.claude/skills/` copies. Consumers that require one of those discovery paths
should copy the canonical directory during local setup. This avoids symlink
portability problems and prevents client-specific copies from drifting.

Both archives are provenance only. The active skill is derived from the latest
`postgresql-sql-format-1.0.0.zip` upload and is versioned `2.0.0` in the
repository to avoid regressing the previous repository-local `1.1.0` version.
Edit the canonical unpacked skill when project decisions change it, update its
version, and document why. Never silently regenerate either original archive.
