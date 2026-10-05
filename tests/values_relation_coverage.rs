mod support;

use semblock::{FormatOptions, format_sql, format_sql_result, validate_equivalent};
use support::assert_sql_layout_only;

fn assert_reviewed(source: &str) {
    let result = format_sql(source, &FormatOptions::default()).expect("VALUES relation formats");
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.fix_available),
        "{:?}",
        result.diagnostics
    );
    assert!(result.warnings.is_empty());
    validate_equivalent(source, &result.output).expect("semantics preserved");
    assert_sql_layout_only(&result.output, &result.output);
}

#[test]
fn grouped_values_sources_and_nested_lateral_queries_are_owned() {
    assert_reviewed(
        "SELECT SUM(summary.total)\nFROM (\n    SELECT labels.label, totals.total\n    FROM (\n        (\n            (VALUES\n                ('red'::text),\n                -- retain the second category\n                ('blue'::text),\n\n                ('green'::text)\n            ) labels(label)\n            JOIN LATERAL (\n                SELECT SUM(records.amount) AS total\n                FROM (\n                    SELECT amount FROM sample_rows WHERE category = labels.label\n                ) records\n            ) totals ON TRUE\n        )\n        JOIN LATERAL (SELECT COUNT(*) AS count FROM sample_rows WHERE category = labels.label) counts ON TRUE\n    )\n    WHERE counts.count > 0\n) summary;",
    );
}

#[test]
fn values_relations_are_shared_by_dml_owners() {
    for source in [
        "UPDATE sample_rows SET amount = source.amount FROM (VALUES (1, 2), (3, 4)) AS source(id, amount) WHERE sample_rows.id = source.id;",
        "DELETE FROM sample_rows USING (VALUES (1), (2)) AS source(id) WHERE sample_rows.id = source.id;",
        "MERGE INTO sample_rows USING (VALUES (1, 2), (3, 4)) AS source(id, amount) ON sample_rows.id = source.id WHEN MATCHED THEN UPDATE SET amount = source.amount;",
    ] {
        assert_reviewed(source);
    }
}

#[test]
fn repeated_nested_values_sources_keep_distinct_owners() {
    assert_reviewed(
        "SELECT first.id, second.id\nFROM (VALUES (1), (2)) AS first(id),\n     (SELECT id FROM (VALUES (3), (4)) AS third(id)) AS second;",
    );
    assert_reviewed("INSERT INTO sample_rows (id) SELECT id FROM (VALUES (1), (2)) AS source(id);");
}

#[test]
fn unsupported_values_shape_reports_its_own_source_location() {
    let source = format!(
        "SELECT 1;\r\n{}SELECT * FROM (\r\n    VALUES (1), (2) ORDER BY 1\r\n) AS source(id);",
        "\r\n".repeat(13)
    );
    let result = format_sql_result(&source, &FormatOptions::default());
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.rule_id == "syntax.unsupported")
        .expect("unreviewed VALUES suffix remains unsupported");
    assert_eq!(
        diagnostic.source_range.start,
        source.find("VALUES").unwrap()
    );
    assert!(
        source[diagnostic.source_range.start..diagnostic.source_range.end].starts_with("VALUES")
    );
    assert_eq!(
        source[..diagnostic.source_range.start]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1,
        16
    );
}

#[test]
fn values_wrapper_and_each_row_use_their_own_layout_width() {
    let source = "SELECT source.label\nFROM (\n    VALUES\n        ('first_label'::sample_schema.sample_label_type),\n        ('second_label'::sample_schema.sample_label_type),\n        ('third_label'::sample_schema.sample_label_type)\n) AS source (label);";
    let result = format_sql(source, &FormatOptions::default()).unwrap();
    assert_eq!(result.output, source);
    assert_reviewed(source);
}

#[test]
fn cli_reports_the_rejected_values_line_and_byte_offset() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let source = format!(
        "-- café\r\n{}SELECT * FROM (\r\n    VALUES (1), (2) ORDER BY 1\r\n) AS source(id);",
        "\r\n".repeat(13)
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_semblock"))
        .args([
            "--stdin",
            "--filename",
            "sample.sql",
            "--language",
            "sql",
            "diff",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let diagnostic = String::from_utf8(output.stderr).unwrap();
    let start = source.find("VALUES").unwrap();
    assert!(
        diagnostic.contains(&format!("sample.sql:16:5 (bytes {start}-")),
        "{diagnostic}"
    );
}

#[test]
fn nested_join_wrappers_do_not_collapse_into_values_wrapper() {
    let source = "SELECT source.id\nFROM (\n    (\n        (\n            VALUES\n                (1),\n                (2)\n        ) AS source (id)\n        JOIN LATERAL (SELECT 1 AS flag) AS flags ON TRUE\n    )\n    JOIN LATERAL (SELECT 2 AS flag) AS other_flags ON TRUE\n);";
    let result = format_sql(source, &FormatOptions::default()).unwrap();
    assert!(
        result
            .output
            .contains("FROM (\n    (\n        (\n            VALUES"),
        "{}",
        result.output
    );
    assert_reviewed(source);
}
