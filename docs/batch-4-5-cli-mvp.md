# Runnable CLI and Go MVP

Status: **complete**

Date: **2026-07-25**

## Outcome

This batch turns the Batch 2 formatter core into a locally usable `semblock`
binary. It deliberately combines the project CLI and raw-Go subset because a
SQL-only CLI would not test the primary embedded-query workflow.

Batch 3 statement layout remains deferred. The CLI does not broaden syntax
claims beyond existing golden fixtures.

## Upstream reuse

The argument-processing and file/stdin flow is adapted from `gmr/pgfmt 2.2.0`
under BSD-3-Clause; its notice is retained in `THIRD_PARTY_NOTICES.md`.

The upstream CLI does not provide recursive discovery, `.gitignore`, custom
ignore files, atomic replacement, diff mode, configuration, directives, or Go
extraction. Those concerns use established crates instead:

| Concern | Selected crate |
| --- | --- |
| CLI contract | `clap 4.6.4` |
| Project traversal and ignore rules | `ignore 0.4.31` |
| Go CST and grammar | `tree-sitter 0.26.11`, `tree-sitter-go 0.25.0` |
| Strict TOML | `serde 1.0.229`, `toml 1.1.3` |
| Unified diff | `similar 3.1.1` |
| Same-directory temporary file | `tempfile 3.27.0` |

No regex crate, second PostgreSQL parser, formatter fork, or parallelism crate
was added.

## Safety flow

For filesystem commands, semblock discovers and sorts every source, reads and
validates all planned outputs, and only then begins `fmt` writes. `check` and
`diff` never write.

Each SQL file:

1. validates directive state;
2. preserves disabled blocks;
3. runs PostgreSQL parse/format/equivalence/idempotence on every active region;
4. preserves a consistent CRLF convention;
5. is replaced through a same-directory temporary file with original
   permissions.

Each Go file:

1. parses the full file with tree-sitter-go;
2. locates supported owner declarations/statements and exact raw-literal
   ranges;
3. attaches directives through adjacent CST comment nodes;
4. classifies only structurally located raw literals;
5. validates every candidate with the PostgreSQL parser;
6. applies in-memory replacements from the end of the file;
7. reparses the complete Go output;
8. passes a second source-formatting idempotence check before any write.

## Deliberate limits

- Go interpreted strings are disabled.
- Direct function-call SQL arguments are not extracted yet.
- SQL `off/on` markers must be standalone lines and delimit complete statement
  regions.
- Hidden paths and symlink traversal are skipped during discovery.
- `.gitignore` rules apply to explicitly traversed directory trees even when
  the tree itself is not a Git repository.
- Unix atomic replacement/permission behavior is integration-tested; other
  supported-platform gates remain release work.
- `--jobs` controls parallel discovery. Formatting remains deterministic and
  sequential.

## Verification

```text
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo doc --locked --no-deps
git diff --check
```

The CLI integration suite covers commands, stdin, strict configuration,
project traversal, nested ignore files, explicit ignored files, SQL and Go
directives, malformed input, no-write paths, whole-project preflight,
whole-Go-file rollback, unified diff, CRLF, permissions, and stable exit
classes. GitHub Actions repeats the complete test suite on Rust 1.88 and current
stable; the MSRV job also enforces formatting, Clippy, documentation, and patch
whitespace.
