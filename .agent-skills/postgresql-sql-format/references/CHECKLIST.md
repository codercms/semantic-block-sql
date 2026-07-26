# Final checklist

- Semantics, order, literals, aliases, identifiers, and meaningful parentheses are unchanged.
- Comments remain attached to the same syntax.
- Keywords and the exact built-in whitelist use uppercase; other functions and types use lowercase.
- Binary operators, commas, casts, function calls, and SQL-grammar parentheses follow the spacing rules.
- Indentation uses four spaces and no tabs.
- Safely breakable lines stay within 160; crossing 120 alone did not force an authored group to split.
- Mixed `AND` / `OR`, nested SQL, and complex parentheses are visible.
- Authored groups, blank lines, and comment boundaries are preserved.
- No line contains only `ON` or `THEN`.
- `CASE` expressions and PL/pgSQL statement blocks use their distinct indentation rules.
- Multiple `EXCEPTION` handlers are separated by blank lines.
- Dollar-quoted PL/pgSQL body roots have no extra `AS $$` indentation.
- Embedded or templated surrounding code is unchanged.
- No optimization, repair, or semantic rewrite was performed.
- Output contains only the requested artifact unless commentary was requested.
