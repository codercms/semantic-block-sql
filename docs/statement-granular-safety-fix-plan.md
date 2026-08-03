# Statement-granular safety and FROM ownership fix plan

Status: complete

1. Add failing fixtures for `IS [NOT] DISTINCT FROM` in SELECT expressions,
   UPDATE assignments, and UPDATE predicates, including a real UPDATE FROM
   clause.
2. Add failing fixtures for set-operation branches that own FROM and named
   WINDOW clauses.
3. Add default and strict policy fixtures for a formatter safety failure in one
   statement of a multi-statement document.
4. Bind FROM only when the typed AST owner requires it, exclude the
   `IS [NOT] DISTINCT FROM` operator sequence, and leave set-operation branch
   clauses to lexical query-block ownership.
5. Represent unsupported and safety-skipped statements as opaque document
   ranges. Preserve those bytes, suppress local style/hard-width checks, and
   continue formatting independent siblings under the default policy.
6. Keep top-level parse/split failures document-fatal and keep strict mode's
   complete-document no-write result.
7. Run focused regressions, the reported real file, the complete Rust gate, and
   GitNexus change-scope review before committing.
