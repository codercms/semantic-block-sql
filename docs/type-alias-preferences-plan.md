# Configurable PostgreSQL type-alias preferences

## Goal

Add disabled-by-default, user-configurable preferred spellings for validated
PostgreSQL built-in type-alias families. Keep arbitrary replacements and
`varchar`/`text` conversion outside the formatter contract.

Work is performed on `feature/type-aliases`, created from fresh `origin/main`.
The original dirty worktree must remain untouched.

## Configuration

```toml
[format.type_aliases]
integer = "int"
character_varying = "varchar"
timestamp_with_time_zone = "timestamptz"
```

Omitted families preserve authored spelling. Unknown families and values are
configuration errors. `check` reports configured mismatches as fixable
`type.alias` diagnostics; `fmt` applies them.

Supported families:

| Family | Allowed spellings |
| --- | --- |
| `smallint` | `smallint`, `int2` |
| `integer` | `integer`, `int`, `int4` |
| `bigint` | `bigint`, `int8` |
| `smallserial` | `smallserial`, `serial2` |
| `serial` | `serial`, `serial4` |
| `bigserial` | `bigserial`, `serial8` |
| `boolean` | `boolean`, `bool` |
| `character` | `character`, `char` |
| `character_varying` | `character varying`, `varchar` |
| `bit_varying` | `bit varying`, `varbit` |
| `numeric` | `numeric`, `decimal` |
| `real` | `real`, `float4` |
| `double_precision` | `double precision`, bare `float`, `float8` |
| `time_with_time_zone` | `time with time zone`, `timetz` |
| `timestamp_without_time_zone` | `timestamp`, `timestamp without time zone` |
| `timestamp_with_time_zone` | `timestamp with time zone`, `timestamptz` |

## Implementation

- Extend typed SQL and PL/pgSQL ownership with parser-owned type spans.
- Normalize configured aliases before layout while mapping diagnostics back to
  authored source ranges; do not add a global keyword scan.
- Preserve modifiers, arrays, comments, authored groups, quoted and qualified
  names, and unsupported statements. Never normalize `float(p)`.
- Validate original-to-normalized changes through an allowlisted alias gate,
  then reuse the existing strict parse/equivalence/protected-token/idempotence
  gates for normalized-to-formatted output.
- Keep the public strict `validate_equivalent` behavior unchanged and add no
  dependency.

## Documentation and example

- Add `examples/semblock-all-type-aliases.toml` with every family enabled and
  validate it in an integration test.
- Update the README, user guide, core specification, formatter design,
  architecture, extension guide, SQL coverage notes, and implementation
  checklist.
- Document that `varchar` and `text` are separate types and are not aliases.
- Document that opting into unqualified shorthand aliases assumes projects do
  not shadow PostgreSQL built-in type names through `search_path`.

## Verification

- Use TDD with fixtures for every family and direction, multi-token aliases,
  serial declarations, diagnostics, widths, SQL routines, PL/pgSQL, and Go.
- Cover default preservation, invalid configuration, modifiers/arrays,
  quoted/qualified/custom types, identifier lookalikes, `float(p)`, comments,
  CRLF mapping, semantic validation, and idempotence.
- Run the complete formatter, Clippy, test, Rustdoc, and diff-hygiene gates.
- Run GitNexus `detect_changes`, complete self-review, update the checklist,
  and commit one coherent batch.
