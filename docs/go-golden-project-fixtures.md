# Go golden-project integration tests

The Go host-language integration suite uses realistic multi-package fixture projects under `tests/fixtures/` rather than isolated string snippets.

The success corpus keeps each input `.go` file beside a complete `.go.expected` golden file. The integration test copies the project, runs the compiled `semblock` CLI, compares complete files byte-for-byte, verifies a second formatting pass is idempotent, checks the clean project with `semblock check`, runs `gofmt`, and compiles the project offline with `go test ./...`.

The corpus covers:

- package-level and grouped `const` declarations;
- package-level `var` declarations and raw SQL nested in function-call arguments;
- local constants, short declarations, and regular assignments;
- direct `return` statements containing database calls;
- standalone call expression statements;
- SELECT, INSERT/ON CONFLICT, UPDATE, DELETE, comments, quoted identifiers, and backslash-containing literals;
- interpreted strings, incomplete fragments, explicit ignore directives, and project ignore files that must remain unchanged.

A separate invalid project proves project-wide preflight: malformed embedded SQL prevents every planned Go rewrite, including otherwise valid files.

Direct `return` and standalone expression statements are structural Go owners. Detection remains database-library-neutral: a raw string is considered only after Tree-sitter locates it under an explicitly supported owner, and PostgreSQL parsing remains the SQL authority.
