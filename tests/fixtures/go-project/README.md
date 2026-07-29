# Go integration fixture

Each `.go` source file has an adjacent `.go.expected` file containing the complete expected source after `semblock fmt`.

The Rust integration harness copies this directory to a temporary project, checks the initial changed-file set, formats it through the compiled CLI, compares complete files byte-for-byte, verifies idempotence across job counts, runs `gofmt`, and compiles the module offline with `go test ./...`.

The nested ignore fixture intentionally remains unformatted and unchanged.
