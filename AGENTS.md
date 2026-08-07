# Repository instructions

These instructions apply to the whole repository.

## Source of truth

Read these files before changing formatter behavior:

1. `docs/semantic-block-sql-fmt-check-core-spec.md`
2. `docs/formatter-design.md`
3. `docs/formatter-architecture.md`
4. `docs/formatter-extension-guide.md`
5. `docs/semantic-block-sql-style-guide-ru.md`
6. `.agent-skills/postgresql-sql-format/SKILL.md` and its referenced material
7. `docs/implementation-checklist.md`

The core specification is authoritative for machine `fmt` / `check` behavior.
The agent skill is human-formatting guidance and must not silently broaden the
core contract. When current documents conflict, the latest explicit project
requirement wins. Record the resolution in `docs/formatter-design.md`.

Historical batch notes and the initial work handoff are indexed in
`docs/README.md`. They explain how the project evolved, but they do not override
current specifications or architecture documents.

## Batch gate

- Work batch by batch and keep `docs/implementation-checklist.md` current.
- Commit each coherent completed batch before starting the next batch.
- Do not mix unrelated cleanup into a batch.
- Do not claim support for a PostgreSQL construct without a fixture.

## Architecture

- Do not write a PostgreSQL parser.
- Keep project discovery, directives, SQL formatting, host-language extraction,
  diffing, diagnostics, and rewriting as separate modules.
- Expose one reusable formatter API for CLI, stdin, Go literals, and future IDE
  adapters.
- IDE adapters must remain thin clients of the canonical formatter engine.
- Go source must be parsed with a real CST/AST. Regex may only classify string
  literals already located structurally.
- Prefer small dependency surfaces. Verify version, license, MSRV, activity, and
  architectural fit before adding a crate.
- Extend PostgreSQL support through the typed ownership IR and exhaustive Rust
  enums described in `docs/formatter-extension-guide.md`; do not add global
  keyword scans or permissive fallbacks.

## Semantic and rewrite safety

- Formatting may change whitespace, indentation, line breaks, keyword casing,
  and PostgreSQL `<>` to `!=` only when `not_equal_policy = prefer_bang`.
- Do not reorder or add syntax, rename aliases, add casts, remove meaningful
  parentheses, alter literals, optimize queries, or change comment attachment.
- Preserve authored list line boundaries as soft group hints.
- Treat blank lines and comments as hard group boundaries.
- Never split a token. Exceeding the hard limit is allowed only for an
  indivisible token, string, comment, or quoted identifier.
- Parse before and after formatting, then format again and require idempotence.
- Never partially rewrite a file.
- Apply Go literal replacements from the end of the file and reparse the whole
  Go file before an atomic write.
- Malformed, unmatched, or incorrectly nested directives produce diagnostics;
  never guess.
- Valid PostgreSQL outside the reviewed ownership model must remain byte-identical
  and report `syntax.unsupported`.

## Development workflow

- Use TDD for formatter behavior.
- Add the smallest failing fixture before changing layout behavior.
- Run focused tests during a batch and the full suite at its completion.
- After every batch, self-review:
  - semantic preservation;
  - architecture boundaries;
  - idempotence;
  - comments and authored groups;
  - diagnostics and error handling;
  - atomicity;
  - dependency necessity;
  - dead code.
- Keep public behavior and exit codes documented.
- Remove dead code before completing a batch.
- Preserve all third-party notices required by forked, copied, or vendored code.

## Tooling

- The minimum supported Rust version is 1.88 with edition 2024.
- Run:
  - `cargo fmt --all -- --check`
  - `cargo clippy --locked --all-targets -- -D warnings`
  - `cargo test --locked --all-targets`
  - `cargo doc --locked --no-deps`
  - `git diff --check`
- Pin any formatter fork or vendored backend to a reviewed upstream commit.

<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **semantic-block-sql** (2542 symbols, 6018 relationships, 221 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> Index stale? Run `node .gitnexus/run.cjs analyze` from the project root — it auto-selects an available runner. No `.gitnexus/run.cjs` yet? `npx gitnexus analyze` (npm 11 crash → `npm i -g gitnexus`; #1939).

## Always Do

- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows. For regression review, compare against the default branch: `detect_changes({scope: "compare", base_ref: "main"})`.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `query({search_query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `context({name: "symbolName"})`.
- For security review, `explain({target: "fileOrSymbol"})` lists taint findings (source→sink flows; needs `analyze --pdg`).

## Never Do

- NEVER edit a function, class, or method without first running `impact` on it.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `rename` which understands the call graph.
- NEVER commit changes without running `detect_changes()` to check affected scope.

## Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/semantic-block-sql/context` | Codebase overview, check index freshness |
| `gitnexus://repo/semantic-block-sql/clusters` | All functional areas |
| `gitnexus://repo/semantic-block-sql/processes` | All execution flows |
| `gitnexus://repo/semantic-block-sql/process/{name}` | Step-by-step execution trace |

## CLI

| Task | Read this skill file |
|------|---------------------|
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->
