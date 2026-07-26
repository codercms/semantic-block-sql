# Repository instructions

These instructions apply to the whole repository.

## Source of truth

Read these files before changing behavior:

1. `docs/semantic-block-sql-fmt-check-core-spec.md`
2. `docs/formatter-design.md`
3. `docs/semantic-block-sql-work-handoff.md`
4. `docs/semantic-block-sql-style-guide-ru.md`
5. `.agent-skills/postgresql-sql-format/SKILL.md` and its referenced material
6. `docs/implementation-checklist.md`

The core specification is authoritative for machine `fmt` / `check` behavior.
The agent skill is human-formatting guidance and must not silently broaden the
core contract. When other documents conflict, the latest explicit project
requirement wins. Record the resolution in `docs/formatter-design.md`.

## Batch gate

- Work batch by batch and keep `docs/implementation-checklist.md` current.
- Do not begin formatter implementation before Batch 0 is committed.
- Commit each coherent completed batch before starting the next batch.
- Do not mix unrelated cleanup into a batch.
- Do not claim support for a PostgreSQL construct without a fixture.

## Architecture

- Do not write a PostgreSQL parser.
- Investigate `libpgfmt` first. Record concrete limitations before choosing a
  different backend.
- Keep project discovery, directives, SQL formatting, host-language extraction,
  diffing, diagnostics, and rewriting as separate modules.
- Expose one reusable formatter API for CLI, stdin, Go literals, and future IDE
  adapters.
- IDE adapters must remain thin clients of the canonical formatter engine.
- Go source must be parsed with a real CST/AST. Regex may only classify string
  literals already located structurally.
- Prefer small dependency surfaces. Verify version, license, MSRV, activity, and
  architectural fit before adding a crate.

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
- The project license is MIT, as selected in the repository's initial commit.
  Preserve all third-party notices required by any forked or vendored code.

## Tooling

- The minimum Rust version is provisional until Batch 1, but the current
  preferred backend requires Rust 1.88 and edition 2024.
- Use `cargo fmt`, `cargo clippy`, and focused `cargo test` targets once Rust
  code exists.
- Pin any formatter fork or vendored backend to a reviewed upstream commit.
