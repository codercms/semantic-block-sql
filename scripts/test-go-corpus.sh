#!/usr/bin/env sh
set -eu
cargo run --locked --example go_corpus -- --report target/go-corpus-report.json "$@"
