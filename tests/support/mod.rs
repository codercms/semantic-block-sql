#![allow(dead_code)]

use pretty_assertions::assert_eq;
use semblock::{FormatOptions, check_sql, format_sql, format_sql_result, validate_equivalent};

#[derive(Debug, Clone, Copy)]
pub struct SqlCase<'a> {
    pub name: &'a str,
    pub input: &'a str,
    pub expected: &'a str,
}

impl<'a> SqlCase<'a> {
    pub const fn new(name: &'a str, input: &'a str, expected: &'a str) -> Self {
        Self {
            name,
            input,
            expected,
        }
    }
}

pub fn assert_sql(input: &str, expected: &str) {
    assert_sql_named("SQL case", input, expected, &FormatOptions::default());
}

pub fn assert_sql_with(input: &str, expected: &str, options: &FormatOptions) {
    assert_sql_named("SQL case", input, expected, options);
}

pub fn assert_cases(cases: &[SqlCase<'_>]) {
    assert_cases_with(cases, &FormatOptions::default());
}

pub fn assert_cases_with(cases: &[SqlCase<'_>], options: &FormatOptions) {
    for case in cases {
        assert_sql_named(case.name, case.input, case.expected, options);
    }
}

pub fn assert_sql_layout_only(input: &str, expected: &str) {
    assert_sql_layout_only_named("SQL case", input, expected, &FormatOptions::default());
}

fn assert_sql_layout_only_named(name: &str, input: &str, expected: &str, options: &FormatOptions) {
    let formatted = format_sql(input, options)
        .unwrap_or_else(|error| panic!("{name}: formatting failed: {error:?}\nsource:\n{input}"));
    assert_eq!(formatted.output, expected, "{name}: formatted output");
    assert!(
        formatted.warnings.is_empty(),
        "{name}: unexpected warnings: {:?}",
        formatted.warnings,
    );
    let second = format_sql(expected, options)
        .unwrap_or_else(|error| panic!("{name}: idempotence pass failed: {error:?}"));
    assert_eq!(
        second.output, expected,
        "{name}: formatting must be idempotent"
    );
    assert!(
        check_sql(expected, options).compliant,
        "{name}: expected SQL must pass check",
    );
}

pub fn assert_fixture_pair(directory: &str, name: &str) {
    let input = std::fs::read_to_string(format!("tests/fixtures/{directory}/{name}.input.sql"))
        .unwrap_or_else(|error| panic!("{directory}/{name}: fixture input: {error}"));
    let expected =
        std::fs::read_to_string(format!("tests/fixtures/{directory}/{name}.expected.sql"))
            .unwrap_or_else(|error| panic!("{directory}/{name}: fixture expectation: {error}"));
    assert_sql_named(name, &input, &expected, &FormatOptions::default());
}

pub fn assert_unsupported(source: &str) {
    assert_unsupported_named("unsupported SQL", source);
}

pub fn assert_unsupported_cases(cases: &[(&str, &str)]) {
    for (name, source) in cases {
        assert_unsupported_named(name, source);
    }
}

fn assert_sql_named(name: &str, input: &str, expected: &str, options: &FormatOptions) {
    let formatted = format_sql(input, options)
        .unwrap_or_else(|error| panic!("{name}: formatting failed: {error:?}\nsource:\n{input}"));
    assert_eq!(formatted.output, expected, "{name}: formatted output");
    assert!(
        formatted.warnings.is_empty(),
        "{name}: unexpected warnings: {:?}",
        formatted.warnings,
    );
    validate_equivalent(input, expected)
        .unwrap_or_else(|error| panic!("{name}: semantic equivalence failed: {error:?}"));

    let second = format_sql(expected, options)
        .unwrap_or_else(|error| panic!("{name}: idempotence pass failed: {error:?}"));
    assert_eq!(
        second.output, expected,
        "{name}: formatting must be idempotent"
    );
    assert!(
        second.warnings.is_empty(),
        "{name}: idempotence pass emitted warnings: {:?}",
        second.warnings,
    );

    let checked = check_sql(expected, options);
    assert!(
        checked.compliant,
        "{name}: expected SQL must pass check: {:?}",
        checked.diagnostics,
    );
}

fn assert_unsupported_named(name: &str, source: &str) {
    let result = format_sql_result(source, &FormatOptions::default());
    assert_eq!(result.output, source, "{name}: unsupported SQL changed");
    assert!(!result.changed, "{name}: unsupported SQL reported a change");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.rule_id == "syntax.unsupported"),
        "{name}: expected syntax.unsupported, got {:?}",
        result.diagnostics,
    );
}
