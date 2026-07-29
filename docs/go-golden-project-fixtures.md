# Go golden-project integration tests

The Go host-language integration suite uses realistic multi-package fixture projects under `tests/fixtures/` rather than isolated string snippets.

The success corpus keeps each input `.go` file beside a complete `.go.expected` golden file. The integration test copies the project, runs the compiled `semblock` CLI, compares complete files byte-for-byte, verifies serial and parallel formatting produce identical trees, checks idempotence, runs `gofmt`, and compiles the project offline with `go test ./...`.

The corpus covers:

- package-level and grouped declarations;
- local constants, variables, assignments, and nested call arguments;
- direct `return` and standalone expression-statement owners;
- SQL-root indentation inside multiline raw strings;
- concatenated SQL fragments that must be skipped conservatively;
- representative supported PostgreSQL statements, comments, quoted identifiers, and backslash-containing literals;
- interpreted strings, explicit directives, and project ignore files that must remain unchanged;
- CRLF preservation and deterministic `--jobs 1` versus `--jobs 4` output.

Separate malformed-SQL, malformed-Go, and directive-error projects prove project-wide preflight: one invalid host file prevents every planned rewrite.

Detection remains structural and database-library-neutral. Tree-sitter identifies an explicitly supported Go owner and PostgreSQL parsing remains the SQL authority.
