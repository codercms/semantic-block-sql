mod support;

use semblock::{FormatOptions, Severity, UnsupportedPolicy, format_sql};
use support::{assert_sql_layout_only, assert_unsupported};

#[test]
fn preserves_comments_and_blank_groups_in_procedural_cte() {
    let sql = "DO $$\nBEGIN\n    WITH seed AS (\n        SELECT 1 AS id\n    )\n    SELECT\n        id,\n\n        -- Keep this field group.\n        id AS copied_id\n    FROM seed;\nEND;\n$$;";
    assert_sql_layout_only(sql, sql);
}

#[test]
fn nested_unsupported_sql_diagnostics_survive_the_procedural_adapter() {
    let statement = "WITH seed AS (SELECT json_value(payload, '$.id') AS id FROM sample_rows) SELECT id FROM seed;";
    let source = format!("DO $$\nBEGIN\n    {statement}\nEND;\n$$;");
    for policy in [UnsupportedPolicy::Skip, UnsupportedPolicy::Error] {
        let options = FormatOptions {
            unsupported_policy: policy,
            ..FormatOptions::default()
        };
        let result = format_sql(&source, &options).expect("safe unsupported SQL returns a result");
        let diagnostic = result
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.rule_id == "syntax.unsupported")
            .expect("the SQL leaf diagnostic must reach the caller");
        assert_eq!(
            &source[diagnostic.source_range.start..diagnostic.source_range.end],
            statement
        );
        assert_eq!(
            diagnostic.severity,
            if policy == UnsupportedPolicy::Error {
                Severity::Error
            } else {
                Severity::Warning
            }
        );
        assert_eq!(result.output, source);
        assert!(!result.changed);
    }
}

#[test]
fn procedural_keywords_inside_quoted_assignment_targets_remain_identifiers() {
    let sql =
        "DO $$\nDECLARE\n    \"return\" integer;\nBEGIN\n    \"return\" = (SELECT 1);\nEND;\n$$;";
    assert_sql_layout_only(sql, sql);
}

#[test]
fn unknown_procedural_parser_nodes_remain_unsupported() {
    assert_unsupported("DO $$ BEGIN CALL sample_work(); END; $$;");
}

#[test]
fn indivisible_procedural_literals_warn_without_skipping_the_routine() {
    let literal = "x".repeat(200);
    let source = format!("DO $$\nBEGIN\n    PERFORM '{literal}';\nEND;\n$$;");
    let result = format_sql(&source, &FormatOptions::default()).expect("literal cannot be split");
    assert_eq!(result.output, source);
    assert!(!result.warnings.is_empty());
    assert!(!result.diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.rule_id.as_str(),
            "syntax.unsupported" | "format.statement_skipped"
        )
    }));
    assert_sql_layout_only(
        "DO $$\nBEGIN\n    PERFORM 'short';\nEND;\n$$;",
        "DO $$\nBEGIN\n    PERFORM 'short';\nEND;\n$$;",
    );
}
