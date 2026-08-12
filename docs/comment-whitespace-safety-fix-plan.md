# Comment trailing-whitespace safety fix

Status: **Complete**

## Goal

Trailing spaces and tabs on physical comment lines are formatting whitespace.
They must be normalized without causing a protected-token safety failure or a
statement skip. All other comment bytes, attachment, syntax, and line endings
remain protected.

## Architecture decisions

- Normalize Unicode whitespace (`char::is_whitespace`) other than CR/LF before
  LF, CRLF, bare CR, and at comment EOF. This includes ASCII space/tab and
  Unicode separators while preserving non-whitespace format characters.
- Apply the rule to `--` comments and every physical line of `/* ... */`
  comments.
- Keep writer-owned whitespace pending until a token is emitted; newline and
  finish operations may discard pending layout whitespace but never trim token
  bytes already written.
- Compare comments through the same canonical trailing-whitespace rule in the
  protected-token equivalence gate.
- Emit exact, fixable `spacing.trailing_whitespace` diagnostics for normalized
  source byte runs.
- Preserve the public formatter API. Internally carry a trusted source range for
  protected-token mismatches so `format.statement_skipped` can point at the
  cause; retain the whole-statement fallback when no exact range exists.

## Test matrix

- Standalone and inline line comments in top-level, CTE, and nested-query
  contexts.
- ASCII and Unicode whitespace with LF, CRLF, bare CR, and EOF.
- Multiline block comments with more than one normalized physical line.
- Multibyte Unicode trailing whitespace with exact UTF-8 byte-range assertions.
- Negative controls for meaningful internal comment whitespace, literals,
  quoted identifiers, and dollar-quoted strings.
- Unsupported/skipped statements remain byte-identical.
- Structural equivalence, idempotence, clean formatted check, and full project
  quality gates.

## Completion criteria

A synthetic reproduction of the reported behavior formats normally, exposes
ordinary style diagnostics, and no longer emits `format.statement_skipped`.
Production SQL is not copied into fixtures. The implementation checklist is
fully checked, documentation records the policy resolution, and the batch is
committed coherently.
