# SQL formatter test organization

The test suite separates **feature examples** from **cross-cutting policy matrices**.
This keeps regressions small enough to review while still exercising every owner that
shares formatter logic.

## Shared assertions

`tests/support/mod.rs` contains the common success and unsupported-syntax contracts.
A successful SQL case checks, in one place:

1. exact formatted output;
2. absence of formatter warnings;
3. PostgreSQL structural equivalence when applicable;
4. second-pass byte idempotence;
5. clean `check` output.

PL/pgSQL bodies and protected `COPY FROM STDIN` payloads use the layout-only variant,
because the outer PostgreSQL structural comparator intentionally does not compare
those embedded languages byte-for-byte.

## Test categories

- `batch*.rs`: statement-family and historical feature coverage. These files retain
  their existing names so earlier regressions remain easy to trace.
- `coverage_layout_matrix.rs`: the shared compact/expanded policy across every list
  and predicate owner.
- `coverage_joins.rs`: every PostgreSQL join header, relation-source context, and
  join-constraint boundary.
- `coverage_operators.rs`: high-variation PostgreSQL operator families and contextual
  grammar casing.
- `coverage_set_operations.rs`: recursive set-operation ownership, wrappers, suffixes,
  and query containers.
- `coverage_support_boundaries.rs`: reviewed syntax paired with adjacent valid but
  intentionally unsupported PostgreSQL forms.
- fixture directories: only cases where comments, blank lines, protected payloads,
  host-language envelopes, or long procedural structures are themselves under test.

## Adding a regression

Prefer the smallest named inline `SqlCase` that reproduces one invariant. Use a
fixture pair only when inline string escapes would obscure the layout being tested.
Do not copy an entire production migration when a focused statement reproduces the
same owner boundary.

When a bug affects shared policy, add sibling cases to the relevant matrix rather
than another one-off file. A newly supported PostgreSQL feature should include:

- a canonical formatting case;
- semantic-equivalence and idempotence through the shared helper;
- a neighboring valid-but-unreviewed form that remains byte-identical, when such a
  boundary exists.
