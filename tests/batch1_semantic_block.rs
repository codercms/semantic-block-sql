use pretty_assertions::assert_eq;
use semblock::{FormatDiagnostic, FormatOptions, Style, format_sql, validate_equivalent};

fn format(source: &str) -> String {
    let formatted = format_sql(source, &FormatOptions::default()).expect("format succeeds");
    assert_eq!(
        format_sql(&formatted.output, &FormatOptions::default())
            .expect("second format succeeds")
            .output,
        formatted.output,
        "fixture must be idempotent"
    );
    formatted.output
}

#[test]
fn compact_select_casing_functions_types_and_operator() {
    assert_eq!(
        format(include_str!("fixtures/batch1/compact-select.input.sql")),
        include_str!("fixtures/batch1/compact-select.expected.sql")
    );
}

#[test]
fn mixed_boolean_expression_exposes_precedence() {
    assert_eq!(
        format(include_str!("fixtures/batch1/mixed-boolean.input.sql")),
        include_str!("fixtures/batch1/mixed-boolean.expected.sql")
    );
}

#[test]
fn simple_join_stays_inline_without_connector_indent() {
    assert_eq!(
        format(include_str!("fixtures/batch1/simple-join.input.sql")),
        include_str!("fixtures/batch1/simple-join.expected.sql")
    );
}

#[test]
fn complex_join_uses_owner_line_on_layout() {
    assert_eq!(
        format(include_str!("fixtures/batch1/complex-join.input.sql")),
        include_str!("fixtures/batch1/complex-join.expected.sql")
    );
}

#[test]
fn comments_and_dollar_strings_are_byte_preserved() {
    let source = include_str!("fixtures/batch1/comments.input.sql");
    let output = format(source);

    assert!(output.contains("-- query purpose"));
    assert!(output.contains("/* stable attachment */"));
    assert!(output.contains("$$literal SELECT and <>$$"));
    validate_equivalent(source, &output).expect("protected tokens and AST remain equivalent");
}

#[test]
fn multiple_complete_statements_remain_in_order() {
    assert_eq!(
        format(include_str!(
            "fixtures/batch1/multiple-statements.input.sql"
        )),
        include_str!("fixtures/batch1/multiple-statements.expected.sql")
    );
}

#[test]
fn invalid_postgresql_is_rejected_before_layout() {
    let error = format_sql("SELECT FROM;", &FormatOptions::default()).unwrap_err();
    assert!(matches!(error, FormatDiagnostic::PostgreSqlParse(_)));
}

#[test]
fn semantic_validation_rejects_literal_or_order_changes() {
    assert_eq!(
        validate_equivalent("SELECT 1;", "SELECT 2;"),
        Err(FormatDiagnostic::SemanticMismatch)
    );
    assert_eq!(
        validate_equivalent("SELECT a, b FROM t;", "SELECT b, a FROM t;"),
        Err(FormatDiagnostic::SemanticMismatch)
    );
}

#[test]
fn options_are_validated() {
    let invalid = FormatOptions {
        style: Style::SemanticBlock,
        indent_width: 4,
        soft_line_width: 160,
        hard_line_width: 120,
        ..FormatOptions::default()
    };
    assert!(matches!(
        format_sql("SELECT 1;", &invalid),
        Err(FormatDiagnostic::InvalidOptions(_))
    ));
}
