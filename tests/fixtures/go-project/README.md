# Go integration fixture

Each `.go` source file has an adjacent `.go.expected` file containing the complete expected source after `semblock fmt`.

The Rust integration harness copies this project, checks the initial changed-file set, compares serial and parallel formatting, verifies byte-for-byte goldens and idempotence, runs `gofmt`, and compiles the module offline with `go test ./...`.

The `ignored/` fixture intentionally remains unformatted and unchanged through `.semblockignore`.
