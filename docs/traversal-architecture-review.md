# Traversal and coordinate architecture review

Status: implementation in progress; JOIN-width behavior change is deferred.

## Findings

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
- [ ] Extract shared layout owner/line geometry operations, migrate duplicate
  calculations and verify contextual indentation through nested combinations.
- [ ] Re-audit traversal callers and remove obsolete helpers. Run full gates,
  private-copy audits, and update architecture/extension documentation.

Every completed batch is committed separately. Formatting behavior stays
covered by equivalence, comment/literal preservation, idempotence and atomicity
gates. The JOIN-width expansion policy is a subsequent behavior batch.
