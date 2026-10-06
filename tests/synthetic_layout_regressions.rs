mod support;

use semblock::{
    FormatOptions, Severity, UnsupportedPolicy, check_sql, format_sql, validate_equivalent,
};
use support::{assert_sql, assert_sql_layout_only};

// Deliberately small, invented SQL: these tests contain no production corpus.
#[test]
fn nested_derived_queries_gain_one_indent_per_wrapper() {
    let sql = "SELECT outer_row.id\nFROM (\n    SELECT middle_row.id\n    FROM (\n        SELECT inner_row.id\n        FROM (\n            SELECT id\n            FROM sample_rows\n        ) inner_row\n    ) middle_row\n) outer_row;";
    assert_sql(sql, sql);
}

#[test]
fn nested_select_items_and_clauses_share_their_owner_indent() {
    let sql = "SELECT outer_row.id\nFROM (\n    SELECT middle_row.id\n    FROM (\n        SELECT inner_row.id\n        FROM (\n            SELECT\n                id,\n\n                -- Keep the authored group.\n                label\n            FROM sample_rows\n            WHERE enabled = TRUE\n        ) inner_row\n    ) middle_row\n) outer_row;";
    assert_sql(sql, sql);
}

#[test]
fn grouped_lateral_sources_keep_nested_query_indentation() {
    let sql = "SELECT result.id\nFROM (\n    SELECT detail.id\n    FROM (\n        (\n            SELECT *\n            FROM sample_rows base\n            JOIN LATERAL (\n                SELECT nested.id\n                FROM (\n                    SELECT id\n                    FROM sample_details\n                ) nested\n            ) detail ON TRUE\n        )\n        JOIN LATERAL (SELECT COUNT(*) AS total) tally ON TRUE\n    )\n) result;";
    assert_sql(sql, sql);
}

fn assert_procedural_with(statement: &str) {
    // The SQL engine already owns WITH + DML. Its procedural client should
    // produce the same layout with the body's four-space width budget.
    let nested_options = FormatOptions {
        soft_line_width: 116,
        hard_line_width: 156,
        ..FormatOptions::default()
    };
    let canonical = format_sql(statement, &nested_options).expect("standalone SQL is supported");
    assert!(
        !canonical.diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.rule_id.as_str(),
                "syntax.unsupported" | "format.statement_skipped"
            )
        }),
        "{:?}",
        canonical.diagnostics
    );
    let body = canonical
        .output
        .lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("    {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let sql = format!("DO $$\nBEGIN\n{body}\nEND;\n$$;");
    assert_sql_layout_only(&sql, &sql);
}

#[test]
fn procedural_with_select_keeps_authored_clauses() {
    assert_procedural_with("WITH seed AS (SELECT 1 AS id)\nSELECT id\nFROM seed;");
}

#[test]
fn procedural_with_insert_keeps_authored_clauses() {
    assert_procedural_with(
        "WITH seed AS (SELECT 1 AS id)\nINSERT INTO sample_rows (id)\nSELECT id\nFROM seed;",
    );
}

#[test]
fn procedural_with_update_keeps_authored_clauses() {
    assert_procedural_with(
        "WITH seed AS (SELECT 1 AS id)\nUPDATE sample_rows\nSET value = seed.id\nFROM seed\nWHERE sample_rows.id = seed.id;",
    );
}

#[test]
fn procedural_with_delete_keeps_authored_clauses() {
    assert_procedural_with(
        "WITH seed AS (SELECT 1 AS id)\nDELETE FROM sample_rows\nUSING seed\nWHERE sample_rows.id = seed.id;",
    );
}

#[test]
fn procedural_with_keeps_breakable_lines_within_hard_width() {
    assert_procedural_with(
        "WITH seed AS (SELECT 1 AS id)\nSELECT\n    id AS first_value,\n    id AS second_value,\n    id AS third_value,\n    id AS fourth_value,\n    id AS fifth_value,\n    id AS sixth_value,\n    id AS seventh_value,\n    id AS eighth_value\nFROM seed;",
    );
}

fn assert_long_routine_header(kind: &str) {
    // Every token is short; the header can safely break at argument boundaries.
    let arguments = (1..=8)
        .map(|index| format!("argument_{index} integer"))
        .collect::<Vec<_>>()
        .join(", ");
    let returns = if kind == "FUNCTION" {
        " RETURNS void"
    } else {
        ""
    };
    let source = format!(
        "CREATE {kind} sample_routine({arguments}){returns} LANGUAGE plpgsql AS $$\nBEGIN\n    PERFORM 1;\nEND;\n$$;"
    );
    assert!(source.lines().next().unwrap().len() > 160);
    let result = format_sql(&source, &FormatOptions::default())
        .expect("a safely breakable routine header must format successfully");
    assert!(result.changed, "{kind}: the header must expand");
    assert!(result.warnings.is_empty(), "{kind}: {:?}", result.warnings);
    assert!(
        !result.diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.rule_id.as_str(),
                "syntax.unsupported" | "format.statement_skipped"
            )
        }),
        "{kind}: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .output
            .lines()
            .all(|line| line.chars().count() <= 160)
    );
    validate_equivalent(&source, &result.output).expect("routine semantics are preserved");
    assert_sql(&result.output, &result.output);
}

#[test]
fn long_function_headers_expand_without_skipping() {
    assert_long_routine_header("FUNCTION");
}

#[test]
fn long_procedure_headers_expand_without_skipping() {
    assert_long_routine_header("PROCEDURE");
}

fn assert_routine_width_failure_is_local(policy: UnsupportedPolicy) {
    // An independently unplanned procedural condition exercises the routine
    // safety gate even after WITH and routine header layout have been fixed.
    let condition = ["TRUE"; 30].join(" AND ");
    let routine = format!(
        "DO $$\nBEGIN\n    IF {condition} THEN\n        PERFORM 1;\n    END IF;\nEND;\n$$;"
    );
    let source = format!("select 1;\n\n{routine}\n\nselect 2;");
    let options = FormatOptions {
        unsupported_policy: policy,
        ..FormatOptions::default()
    };
    let result = format_sql(&source, &options)
        .expect("routine width failure must return a statement-level formatter result");
    let skipped = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.rule_id == "format.statement_skipped");
    if let Some(diagnostic) = skipped {
        assert!(diagnostic.message.contains("above hard limit"));
        assert!(!diagnostic.fix_available);
        assert_eq!(
            &source[diagnostic.source_range.start..diagnostic.source_range.end],
            routine
        );
        if policy == UnsupportedPolicy::Error {
            assert_eq!(diagnostic.severity, Severity::Error);
            assert_eq!(result.output, source);
            assert!(!result.changed);
        } else {
            assert_eq!(diagnostic.severity, Severity::Warning);
            assert_eq!(
                result.output,
                format!("SELECT 1;\n\n{routine}\n\nSELECT 2;")
            );
        }
    } else {
        // A future condition planner may safely format this statement instead.
        assert!(result.output.starts_with("SELECT 1;"));
        assert!(result.output.ends_with("SELECT 2;"));
        assert!(
            result
                .output
                .lines()
                .all(|line| line.chars().count() <= 160)
        );
        assert!(check_sql(&result.output, &options).compliant);
    }
    let second = format_sql(&result.output, &options).expect("the result remains idempotent");
    assert_eq!(second.output, result.output);
}

#[test]
fn routine_width_failure_preserves_sibling_formatting() {
    assert_routine_width_failure_is_local(UnsupportedPolicy::Skip);
}

#[test]
fn strict_routine_width_failure_returns_the_unchanged_document() {
    assert_routine_width_failure_is_local(UnsupportedPolicy::Error);
}
