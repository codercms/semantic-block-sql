mod support;

use pretty_assertions::assert_eq;
use semblock::{FormatOptions, Severity, format_sql, format_sql_result};
use support::assert_sql_layout_only as assert_fixture;

#[test]
fn formats_common_operational_and_migration_utilities() {
    assert_fixture(
        include_str!("fixtures/batch14/utilities.input.sql"),
        include_str!("fixtures/batch14/utilities.expected.sql"),
    );
}

#[test]
fn formats_copy_stdin_header_and_preserves_payload_byte_for_byte() {
    let source = "copy public.items(id,name) from stdin with(format csv);\n1,Alice\n2,\"Bob, Jr.\"\n\\.\nselect id,name from public.items;\n";
    let expected = "COPY public.items(id, name) FROM STDIN WITH (FORMAT csv);\n1,Alice\n2,\"Bob, Jr.\"\n\\.\nSELECT id, name FROM public.items;\n";

    assert_fixture(source, expected);
    let payload = "1,Alice\n2,\"Bob, Jr.\"\n\\.\n";
    assert!(
        format_sql(source, &FormatOptions::default())
            .expect("format copy payload")
            .output
            .contains(payload)
    );
}

#[test]
fn hard_width_errors_after_copy_payload_use_document_line_numbers() {
    let source = "COPY public.items(id) FROM STDIN;\n1\n\\.\nALTER TABLE public.long_table_name ALTER COLUMN long_column_name SET DEFAULT 123;";
    let options = FormatOptions {
        soft_line_width: 32,
        hard_line_width: 40,
        ..FormatOptions::default()
    };

    let formatted = format_sql(source, &options).expect("the failed statement is preserved");
    assert_eq!(formatted.output, source);
    let diagnostic = formatted
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.rule_id == "format.statement_skipped")
        .expect("the hard-width failure is diagnosed");
    assert!(diagnostic.message.contains("line 4"), "{diagnostic:?}");
}

#[test]
fn formats_plain_top_level_transactions_without_comment_diagnostics() {
    let source = include_str!("fixtures/batch14/transactions.input.sql");
    let expected = include_str!("fixtures/batch14/transactions.expected.sql");

    assert_fixture(source, expected);
    let formatted = format_sql(source, &FormatOptions::default())
        .expect("plain transaction formatting succeeds");
    assert!(
        formatted
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.rule_id != "syntax.unsupported"),
        "{:?}",
        formatted.diagnostics
    );
}

#[test]
fn unreviewed_transaction_controls_remain_unsupported() {
    for source in ["START TRANSACTION;", "COMMIT AND CHAIN;", "ROLLBACK;"] {
        let result = format_sql_result(source, &FormatOptions::default());
        assert_eq!(result.output, source, "{source}");
        assert!(!result.changed, "{source}");
        assert!(
            result.diagnostics.iter().any(|diagnostic| {
                diagnostic.rule_id == "syntax.unsupported"
                    && diagnostic.severity == Severity::Warning
            }),
            "{source}: {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn unreviewed_utility_neighbors_are_preserved_and_non_fatal() {
    for source in [
        "CREATE SCHEMA app CREATE TABLE nested(id bigint);",
        "ALTER EXTENSION pg_trgm UPDATE;",
        "CREATE CAST(text AS uuid) WITHOUT FUNCTION;",
    ] {
        let result = format_sql_result(source, &FormatOptions::default());
        assert_eq!(result.output, source, "{source}");
        assert!(!result.changed, "{source}");
        assert!(
            result.diagnostics.iter().any(|diagnostic| {
                diagnostic.rule_id == "syntax.unsupported"
                    && diagnostic.severity == Severity::Warning
            }),
            "{source}: {:?}",
            result.diagnostics
        );
    }
}
