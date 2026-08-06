use pretty_assertions::assert_eq;
use semblock::{FormatOptions, Severity, check_sql, format_sql, format_sql_result};

#[test]
fn formats_sql_standard_routine_with_default_argument() {
    let source = include_str!("fixtures/sql_standard_routine/function.input.sql");
    let expected = include_str!("fixtures/sql_standard_routine/function.expected.sql");
    let options = FormatOptions::default();

    let formatted = format_sql(source, &options).expect("format succeeds");
    assert_eq!(formatted.output, expected);
    assert_eq!(
        format_sql(expected, &options)
            .expect("idempotent format succeeds")
            .output,
        expected
    );
    assert!(check_sql(expected, &options).compliant);
}

#[test]
fn preserves_unreviewed_multi_statement_sql_routines() {
    let source =
        "CREATE FUNCTION f()\nRETURNS void\nLANGUAGE SQL\nBEGIN ATOMIC\nSELECT 1;\nSELECT 2;\nEND;";
    let result = format_sql_result(source, &FormatOptions::default());

    assert_eq!(result.output, source);
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.severity == Severity::Warning && diagnostic.rule_id == "syntax.unsupported"
    }));
}
