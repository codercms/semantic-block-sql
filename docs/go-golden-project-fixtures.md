# Go golden-project integration tests

The Go host-language integration suite uses realistic multi-package fixture projects under `tests/fixtures/` rather than isolated string snippets.

The success corpus keeps each input `.go` file beside a complete `.go.expected` golden file. The integration test copies the project, runs the compiled `semblock` CLI, compares complete files byte-for-byte, verifies serial and parallel formatting produce identical trees, checks idempotence, runs `gofmt`, and compiles the project offline with `go test ./...`.

The corpus covers:

- package-level and grouped declarations;
- local constants, variables, assignments, returns, and standalone calls;
- raw and interpreted strings in direct/nested call arguments, `defer`, and `go` calls;
- struct/composite fields, map/slice values, and table-driven test cases;
- literal-only static concatenations and untouched dynamic expressions;
- lossless interpreted-to-raw conversion and escaped interpreted fallback;
- SQL-root indentation inside multiline raw strings;
- representative PostgreSQL migrations, queries, PL/pgSQL, comments, identifiers, and protected literals;
- explicit directives, project ignore files, CRLF preservation, and deterministic `--jobs 1` versus `--jobs 4` output.

Separate malformed-SQL, malformed-Go, and directive-error projects prove project-wide preflight: one invalid host file prevents every planned rewrite.

Detection remains structural and database-library-neutral. Tree-sitter identifies eligible string-expression contexts, a Go codec proves decoded runtime values, and PostgreSQL parsing remains the SQL authority. `tests/corpus/go-projects.json` and the opt-in `go_corpus` example extend this with pinned external projects without adding network access to normal CI.
