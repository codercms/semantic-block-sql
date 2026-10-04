# Rust integration fixture

This edition-2024 Cargo project mirrors the SQL in `../go-project/`, including
the SQL in Go test cases. Each `.rs` input has a complete `.rs.expected` golden.
The host parity test compares the decoded SQL multisets of both projects before
and after formatting, including Go's literal-only concatenation as one value.

The CLI integration copies the project, verifies check/diff do not write,
compares serial/parallel formatting, checks all goldens, ignored files and
idempotence, then runs locked offline `cargo check` and `cargo test`.
The project has no external dependencies and requires no database.

Host contexts differ naturally: Rust uses constants, statics, assignments,
calls, threads and struct/array values. Go's static concatenation is represented
by its identical complete SQL value; dynamic SQL remains opaque in both hosts.
