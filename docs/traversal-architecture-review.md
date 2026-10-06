# Traversal and coordinate architecture review

Status: foundation implemented and reviewed; JOIN-width behavior follow-up is implemented.

## Original findings

The parser/token/ownership/layout/safety separation remains useful. The repeated
regressions reveal inconsistencies within those boundaries:

- Validation and query capability collection use `walk_complete_tree`, which
  supplements the backend's convenience walker. Type-alias normalization and
  minimum expression locations still use the incomplete backend walk directly.
- The supplemented walk combines breadth-first traversal with recursive omitted
  children. Its order is neither uniform depth-first nor uniform breadth-first;
  it does not expose stable parent ownership.
- The backend walker omits expression fields such as aggregate FILTER/order,
  CASE operands and SubLink test expressions, beyond the manually supplemented
  DML/DDL fields. New omissions currently need another special case.
- Query display indentation starts from delimiter depth plus predicate-keyword
  counts, then receives contextual rebasing. These are different coordinate
  systems and must not act as competing authorities.
- Width measurement consistently renders tokens, but callers independently
  select the prefix/indentation budget. The remaining JOIN-width issue is an
  omitted owner prefix, not a failure of PostgreSQL AST parsing.
- SQL and procedural equivalence duplicate source-location stripping. Source
  lines are calculated from text, while recursive adapters manually compose
  input/output offsets. AST depth cannot replace that source mapping.

## Required shared contracts

1. **AST structure:** one deterministic, complete, borrowed depth-first traversal
   of the pinned protobuf schema. Child enumeration is structural; capability
   validation remains a separate fail-closed operation. It must not infer SQL
   grammar or clone/mutate the parser tree. Similar statement families reuse
   the same optional-child, child-list and expression traversal blocks.
2. **Text coordinates:** one immutable UTF-8 source index for line/column and
   byte-range questions. Each parsed/normalized document has its own source
   frame. Original and formatted coordinates must remain explicitly distinct.
3. **Token structure:** retain the syntax-neutral delimiter index. Delimiter
   depth answers lexical nesting questions, not display indentation.
4. **Layout coordinates:** displayed owner indentation and the rendered owner
   prefix determine child indentation and width. Shared planned-line queries
   must replace independently reconstructed prefixes. Existing signed rebasing
   is the sole operation for relocating already-planned children.

A single authority per coordinate system is safer than one universal “depth”
value. Line numbers, AST nesting, delimiter depth and displayed indentation are
not interchangeable. Unsupported/protected spans retain their source frame and
are not reinterpreted by layout consumers.

## Batches and acceptance

- [x] Review/pin pg_query 6.2.1 independently; adapt the PostgreSQL 17.7 version
  gate and new source metadata without broadening supported grammar.
- [x] Replace the supplemented/incomplete AST walks with the canonical child
  traversal; route validation, capability collection, type aliases and source
  anchors through it. Verify ordering, missing child families and unsupported
  neighbors with synthetic fixtures.
- [x] Centralize text line/column indexing and coordinate calculations used by
  diagnostics; cover UTF-8, CRLF, statement prefixes and nested routine adapters.
- [x] Extract shared layout owner/line geometry operations, migrate duplicate
  calculations and verify contextual indentation through nested combinations.
- [x] Re-audit traversal callers and remove obsolete helpers. Run full gates,
  private-copy audits, and update architecture/extension documentation.

Every completed batch is committed separately. Formatting behavior stays
covered by equivalence, comment/literal preservation, idempotence and atomicity
gates. The JOIN-width expansion policy is implemented through shared planned-line geometry.

## Implemented geometry contract and limits

`semantic_block::geometry` owns the layout plan, ordered break positions,
rendered compact widths and planned-line prefix measurement. Predicate opener
and CASE result decisions share this measurement. Relative fallback indentation
uses signed offsets; nested outward/inward rebasing retains the source coordinate
rather than discarding a negative adjustment. Delimiter ancestors are indexed
once and reused for query wrappers and predicate-subquery nesting; sibling
parentheses no longer participate in ancestry discovery.

The backend AST walk is complete for the pinned schema, not a promise of support
for every PostgreSQL construct. Parser decoding still has its own recursion limit.
Display policies remain typed owner-specific rules. JOIN ON predicate layout measures its entire displayed JOIN prefix
through shared planned-line geometry after query/relation owner planning.


Final foundation validation: 513 tests across 65 targets, one existing ignored;
formatting, locked Clippy, Rustdoc, schema reproduction and diff hygiene pass
on Rust 1.88. Both private SQL copies pass fmt/check and byte-identical repeat
formatting in both opener modes without unsupported/skipped/errors. Original
hashes are unchanged. No private SQL or domain-specific fixture is tracked.
Self-review covers semantic preservation, typed ownership/protected spans,
idempotence, comments/authored groups, diagnostics/source frames, atomicity,
dependency necessity and removal of obsolete traversal/coordinate helpers.
