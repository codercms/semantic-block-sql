use semblock::{FormatOptions, check_sql, format_sql_result, validate_equivalent};
mod support;

#[test]
fn wrapped_relations_preserve_comment_and_list_indentation() {
    support::assert_sql(
        "SELECT (SELECT COUNT(*) FROM ((left_rows a JOIN right_rows b ON a.id = b.id) JOIN other_rows c ON c.id = a.id));",
        "SELECT\n    (\n        SELECT COUNT(*)\n        FROM (\n            (\n                left_rows a\n                JOIN right_rows b ON a.id = b.id\n            )\n            JOIN other_rows c ON c.id = a.id\n        )\n    );",
    );
    support::assert_sql(
        "SELECT (SELECT COUNT(*) FROM (\n-- source note\nleft_rows a JOIN right_rows b ON a.id = b.id));",
        "SELECT\n    (\n        SELECT COUNT(*)\n        FROM (\n            -- source note\n            left_rows a\n            JOIN right_rows b ON a.id = b.id\n        )\n    );",
    );
    support::assert_sql(
        "SELECT (SELECT COUNT(*) FROM (left_rows a JOIN right_rows b ON a.id = b.id), (other_rows c JOIN final_rows d ON c.id = d.id));",
        "SELECT\n    (\n        SELECT COUNT(*)\n        FROM\n            (\n                left_rows a\n                JOIN right_rows b ON a.id = b.id\n            ),\n            (\n                other_rows c\n                JOIN final_rows d ON c.id = d.id\n            )\n    );",
    );
}

#[test]
fn wrapped_join_sources_follow_their_nested_query_indent() {
    for source in [
        "SELECT (SELECT COUNT(*) FROM (left_rows a JOIN right_rows b ON a.id = b.id));",
        "CREATE VIEW sample_view AS SELECT (SELECT jsonb_agg(to_jsonb(d.*)) FROM (SELECT a.id, jsonb_build_object('label', b.label) AS detail FROM (link_rows a JOIN label_rows b ON a.label_id = b.id) WHERE a.root_id = root.id ORDER BY a.rank LIMIT 5) d) FROM root_rows root;",
        "SELECT root.id FROM root_rows root JOIN LATERAL (SELECT (SELECT COUNT(*) FROM (left_rows a JOIN right_rows b ON a.id = b.id))) summary ON TRUE;",
    ] {
        let options = FormatOptions::default();
        let result = format_sql_result(source, &options);
        assert!(
            result.diagnostics.iter().all(|d| d.fix_available),
            "{:?}",
            result.diagnostics
        );
        let lines = result.output.lines().collect::<Vec<_>>();
        let mut wrapped_sources = 0;
        for (index, line) in lines.iter().enumerate() {
            if line.trim() != "FROM ("
                || !["left_rows", "link_rows"]
                    .iter()
                    .any(|prefix| lines[index + 1].trim().starts_with(prefix))
            {
                continue;
            }
            wrapped_sources += 1;
            let indent = line.len() - line.trim_start().len();
            assert_eq!(
                lines[index + 1].len() - lines[index + 1].trim_start().len(),
                indent + 4,
                "{}",
                result.output
            );
            assert_eq!(
                lines[index + 2].len() - lines[index + 2].trim_start().len(),
                indent + 4,
                "{}",
                result.output
            );
            assert_eq!(
                lines[index + 3].len() - lines[index + 3].trim_start().len(),
                indent,
                "{}",
                result.output
            );
        }
        assert_eq!(wrapped_sources, 1, "{}", result.output);
        validate_equivalent(source, &result.output).unwrap();
        assert_eq!(
            format_sql_result(&result.output, &options).output,
            result.output
        );
        assert!(check_sql(&result.output, &options).compliant);
    }
}
