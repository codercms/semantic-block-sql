use std::fs;
use std::path::{Path, PathBuf};

use semblock::config::Config;
use semblock::source::{Language, format_source};
use semblock::{FormatOptions, format_sql_result, validate_equivalent};
use tree_sitter::{Node, Parser};

fn sql_fixtures(directory: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            paths.extend(sql_fixtures(&path));
        } else if path.to_string_lossy().ends_with(".input.sql") {
            paths.push(path);
        }
    }
    paths.sort();
    paths
}

fn decoded_literals(source: &str, language: Language) -> Vec<String> {
    fn static_go_value(node: Node<'_>, source: &str) -> Option<String> {
        let text = node.utf8_text(source.as_bytes()).ok()?;
        match node.kind() {
            "raw_string_literal" => Some(text[1..text.len() - 1].replace('\r', "")),
            "interpreted_string_literal" => serde_json::from_str(text).ok(),
            "binary_expression"
                if node
                    .child_by_field_name("operator")?
                    .utf8_text(source.as_bytes())
                    .ok()?
                    == "+" =>
            {
                Some(
                    static_go_value(node.child_by_field_name("left")?, source)?
                        + &static_go_value(node.child_by_field_name("right")?, source)?,
                )
            }
            _ => None,
        }
    }
    fn visit(node: Node<'_>, source: &str, language: Language, values: &mut Vec<String>) {
        if language == Language::Go
            && node.kind() == "binary_expression"
            && let Some(value) = static_go_value(node, source)
        {
            values.push(value);
            return;
        }
        let literal = match language {
            Language::Go => matches!(
                node.kind(),
                "raw_string_literal" | "interpreted_string_literal"
            ),
            Language::Rust => matches!(node.kind(), "raw_string_literal" | "string_literal"),
            _ => unreachable!(),
        };
        if literal {
            let text = node.utf8_text(source.as_bytes()).unwrap();
            let value = match language {
                Language::Go if text.starts_with('`') => text[1..text.len() - 1].replace('\r', ""),
                // These checked-in Go fixtures use only JSON-compatible interpreted escapes.
                Language::Go => serde_json::from_str::<String>(text).unwrap(),
                Language::Rust => match syn::parse_str::<syn::LitStr>(text) {
                    Ok(literal) => literal.value(),
                    Err(_) => return, // Byte/C literals are not SQL string inputs.
                },
                _ => unreachable!(),
            };
            values.push(value);
            return;
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            visit(child, source, language, values);
        }
    }
    let mut parser = Parser::new();
    let grammar = match language {
        Language::Go => tree_sitter_go::LANGUAGE,
        Language::Rust => tree_sitter_rust::LANGUAGE,
        _ => unreachable!(),
    };
    parser.set_language(&grammar.into()).unwrap();
    let tree = parser.parse(source, None).unwrap();
    assert!(!tree.root_node().has_error(), "{source}");
    let mut values = Vec::new();
    visit(tree.root_node(), source, language, &mut values);
    values
}

fn rust_raw(sql: &str) -> String {
    if sql.contains('\r') {
        return format!("{sql:?}");
    }
    let mut hashes = String::from("#");
    while sql.contains(&format!("\"{hashes}")) {
        hashes.push('#');
    }
    format!("r{hashes}\"{sql}\"{hashes}")
}

fn assert_parity(sql: &str, name: &str, options: &FormatOptions) {
    // Remove host indentation before comparing SQL layouts, retaining inner nesting.
    let sql = sql.trim_matches('\n');
    let indent = sql
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .chars()
        .take_while(|c| matches!(c, ' ' | '\t'))
        .collect::<String>();
    let normalized = sql
        .lines()
        .map(|line| line.strip_prefix(&indent).unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");
    let sql = normalized.as_str();
    let canonical = format_sql_result(sql, options);
    let interpreted = serde_json::to_string(sql).unwrap();
    let rust_interpreted = format!("{sql:?}");
    let raw_go = if sql.contains('`') || sql.contains('\r') {
        interpreted.clone()
    } else {
        format!("`{sql}`")
    };
    let raw_rust = rust_raw(sql);
    let sources = [
        (
            Language::Go,
            format!("package fixture\n// semblock:sql\nconst SQL = {raw_go}\n"),
        ),
        (
            Language::Go,
            format!("package fixture\n// semblock:sql\nconst SQL = {interpreted}\n"),
        ),
        (
            Language::Rust,
            format!("// semblock:sql\nconst SQL: &str = {raw_rust};\n"),
        ),
        (
            Language::Rust,
            format!("// semblock:sql\nconst SQL: &str = {rust_interpreted};\n"),
        ),
        (
            Language::Rust,
            format!("fn run() {{\n// semblock:sql\nsqlx::query!({raw_rust});\n}}\n"),
        ),
        (
            Language::Rust,
            format!("fn run() {{\n// semblock:sql\nsqlx::query_as!(Row, {raw_rust});\n}}\n"),
        ),
    ];
    for (language, source) in sources {
        let formatted = format_source(&source, language, options, &Config::default().go)
            .unwrap_or_else(|error| panic!("{name} / {language:?}: {error}\n{source}"));
        let values = decoded_literals(&formatted.output, language);
        assert_eq!(values.len(), 1, "{name} / {language:?}");
        assert_eq!(
            values[0].trim(),
            canonical.output.trim(),
            "{name} / {language:?}"
        );
        let rules = |diagnostics: &[semblock::Diagnostic]| {
            diagnostics
                .iter()
                .filter(|d| {
                    d.rule_id.starts_with("syntax.") || d.rule_id == "format.statement_skipped"
                })
                .map(|d| (d.rule_id.clone(), d.severity))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            rules(&formatted.diagnostics),
            rules(&canonical.diagnostics),
            "{name} / {language:?}"
        );
        if !canonical.changed {
            assert_eq!(values[0], sql, "{name}: unchanged SQL value");
        }
        let parsed = pg_query::parse(sql).unwrap();
        let has_routine = parsed.protobuf.stmts.iter().any(|stmt| {
            matches!(
                stmt.stmt.as_ref().and_then(|node| node.node.as_ref()),
                Some(
                    pg_query::protobuf::node::Node::DoStmt(_)
                        | pg_query::protobuf::node::Node::CreateFunctionStmt(_)
                )
            )
        });
        // The outer PostgreSQL comparator treats routine bodies as protected strings;
        // exact canonical output above covers their separate procedural safety pipeline.
        if !has_routine {
            validate_equivalent(sql, &values[0]).unwrap_or_else(|error| panic!("{name}: {error}"));
        }
        let second =
            format_source(&formatted.output, language, options, &Config::default().go).unwrap();
        assert_eq!(second.output, formatted.output, "{name}: idempotence");
    }
}

#[test]
fn go_and_rust_share_every_existing_sql_fixture() {
    let paths = sql_fixtures(Path::new("tests/fixtures"));
    assert!(paths.len() >= 29, "SQL fixture corpus unexpectedly shrank");
    for path in paths {
        let sql = fs::read_to_string(&path).unwrap();
        assert_parity(&sql, &path.display().to_string(), &FormatOptions::default());
    }
}

#[test]
fn rust_reuses_complete_sql_literals_from_the_go_project() {
    let mut count = 0;
    for path in sql_host_files(Path::new("tests/fixtures/go-project"), "go") {
        let source = fs::read_to_string(&path).unwrap();
        for sql in decoded_literals(&source, Language::Go) {
            if complete_sql(&sql) {
                assert_parity(&sql, &path.display().to_string(), &FormatOptions::default());
                count += 1;
            }
        }
    }
    assert!(count >= 25, "Go SQL corpus unexpectedly shrank: {count}");
}

fn sql_host_files(directory: &Path, extension: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            paths.extend(sql_host_files(&path, extension));
        } else if path.extension().is_some_and(|value| value == extension) {
            paths.push(path);
        }
    }
    paths.sort();
    paths
}

#[test]
fn multiline_go_and_rust_migrations_have_identical_sql_layout() {
    let source =
        fs::read_to_string("tests/fixtures/go-project/internal/migrations/schema.go").unwrap();
    for sql in decoded_literals(&source, Language::Go) {
        let go = format!("package fixture\nconst SQL = `{sql}`\n");
        let rust = format!("const SQL: &str = {};\n", rust_raw(&sql));
        let options = FormatOptions::default();
        let config = Config::default();
        let go = format_source(&go, Language::Go, &options, &config.go).unwrap();
        let rust = format_source(&rust, Language::Rust, &options, &config.go).unwrap();
        assert_eq!(
            decoded_literals(&go.output, Language::Go),
            decoded_literals(&rust.output, Language::Rust)
        );
    }
}

#[test]
fn permanent_projects_have_the_same_input_and_expected_sql_corpus() {
    let corpus = |directory: &str, language: Language, expected: bool| {
        let extension = if language == Language::Go { "go" } else { "rs" };
        let mut sql = Vec::new();
        for path in sql_host_files(Path::new(directory), extension) {
            let path = if expected {
                PathBuf::from(format!("{}.expected", path.display()))
            } else {
                path
            };
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            sql.extend(
                decoded_literals(&source, language)
                    .into_iter()
                    .filter(|value| complete_sql(value)),
            );
        }
        sql.sort();
        sql
    };
    for expected in [false, true] {
        let go = corpus("tests/fixtures/go-project", Language::Go, expected);
        let rust = corpus("tests/fixtures/rust-project", Language::Rust, expected);
        assert!(go.len() >= 30);
        eprintln!(
            "permanent Go/Rust projects: {} SQL expressions, expected={expected}",
            go.len()
        );
        assert_eq!(
            go, rust,
            "permanent project SQL parity, expected={expected}"
        );
    }
}

fn complete_sql(value: &str) -> bool {
    !value.trim().is_empty()
        && !value.trim().eq_ignore_ascii_case("select")
        && pg_query::parse(value).is_ok()
}

#[test]
fn go_and_rust_share_sql_literals_from_the_existing_regression_suite() {
    let mut corpus = std::collections::BTreeSet::new();
    for entry in fs::read_dir("tests").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|extension| extension == "rs") {
            let source = fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
            corpus.extend(
                decoded_literals(&source, Language::Rust)
                    .into_iter()
                    .filter(|value| complete_sql(value)),
            );
        }
    }
    assert!(
        corpus.len() >= 700,
        "regression SQL corpus unexpectedly shrank: {}",
        corpus.len()
    );
    eprintln!(
        "shared Go/Rust SQL regression corpus: {} distinct SQL values",
        corpus.len()
    );
    for sql in corpus {
        assert_parity(&sql, &sql, &FormatOptions::default());
    }
}

#[test]
fn sqlx_compilation_queries_and_offline_metadata_reuse_go_sql() {
    let mut go_sql = std::collections::BTreeSet::new();
    for path in sql_host_files(Path::new("tests/fixtures/go-project"), "go") {
        go_sql.extend(decoded_literals(
            &fs::read_to_string(path).unwrap(),
            Language::Go,
        ));
    }
    // Reuse the scalar SQL from the first Go expression-context regression.
    let source = decoded_literals(include_str!("go_string_coverage.rs"), Language::Rust)
        .into_iter()
        .find(|source| source.starts_with("package "))
        .unwrap();
    go_sql.extend(decoded_literals(&source, Language::Go));
    let root = Path::new("tests/fixtures/rust-sqlx-project");
    let input = decoded_literals(
        &fs::read_to_string(root.join("src/lib.rs")).unwrap(),
        Language::Rust,
    );
    assert_eq!(input.len(), 7);
    for sql in &input {
        assert!(
            go_sql.contains(sql),
            "SQLx query not found in Go corpus: {sql}"
        );
    }
    let expected = decoded_literals(
        &fs::read_to_string(root.join("src/lib.rs.expected")).unwrap(),
        Language::Rust,
    );
    let queries = input
        .into_iter()
        .chain(expected)
        .collect::<std::collections::BTreeSet<_>>();
    let metadata = fs::read_dir(root.join(".sqlx"))
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let value: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            assert_eq!(value["db_name"], "PostgreSQL");
            assert_eq!(
                path.file_name().unwrap().to_str().unwrap(),
                format!("query-{}.json", value["hash"].as_str().unwrap())
            );
            value["query"].as_str().unwrap().to_owned()
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        metadata, queries,
        "offline metadata must cover exactly the input and formatted SQL"
    );
}

#[test]
fn go_and_rust_share_strict_unsupported_boundaries() {
    let options = FormatOptions {
        unsupported_policy: semblock::UnsupportedPolicy::Error,
        ..FormatOptions::default()
    };
    let mut count = 0;
    for sql in decoded_literals(
        include_str!("coverage_support_boundaries.rs"),
        Language::Rust,
    ) {
        if complete_sql(&sql)
            && format_sql_result(&sql, &options)
                .diagnostics
                .iter()
                .any(|d| d.rule_id == "syntax.unsupported")
        {
            assert_parity(&sql, &sql, &options);
            count += 1;
        }
    }
    assert!(
        count >= 10,
        "unsupported boundary corpus unexpectedly shrank: {count}"
    );
}
