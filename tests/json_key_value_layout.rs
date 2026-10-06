mod support;

use semblock::{FormatOptions, check_sql, format_sql_result};
use support::assert_sql;

#[test]
fn aggregate_order_comment_continuations_keep_clause_indentation() {
    for clause in [
        "ORDER BY -- sort\nrank",
        "ORDER -- prefix\nBY rank",
        "ORDER /* prefix\ncontinued */ BY -- sort\nrank",
    ] {
        let source = format!("SELECT jsonb_object_agg(k, v {clause}) FROM sample_rows;");
        let output = reviewed(&source, &FormatOptions::default());
        for line in output
            .lines()
            .filter(|line| matches!(line.trim(), "rank" | "BY rank"))
        {
            assert!(line.starts_with("        "), "{output}");
        }
        assert!(output.contains("        ORDER"), "{output}");
    }
}

fn reviewed(source: &str, options: &FormatOptions) -> String {
    let result = format_sql_result(source, options);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.fix_available || d.rule_id == "layout.hard_line_width"),
        "{:?}",
        result.diagnostics
    );
    let second = format_sql_result(&result.output, options);
    assert_eq!(second.output, result.output);
    assert!(check_sql(&result.output, options).compliant);
    semblock::validate_equivalent(source, &result.output).unwrap();
    result.output
}

#[test]
fn expanded_builders_regroup_keys_and_values() {
    for function in [
        "json_build_object",
        "jsonb_build_object",
        "pg_catalog.jsonb_build_object",
        "\"json_build_object\"",
    ] {
        let source = format!(
            "SELECT\n    {function}(\n        'id',\n        item.id,\n        'title',\n        item.title\n    )\nFROM sample_rows item;"
        );
        let expected = format!(
            "SELECT\n    {function}(\n        'id', item.id,\n        'title', item.title\n    )\nFROM sample_rows item;"
        );
        assert_sql(&source, &expected);
    }
}

#[test]
fn object_aggregate_families_keep_the_single_pair_together() {
    for function in [
        "json_object_agg",
        "jsonb_object_agg",
        "json_object_agg_strict",
        "jsonb_object_agg_strict",
        "json_object_agg_unique",
        "jsonb_object_agg_unique",
        "json_object_agg_unique_strict",
        "jsonb_object_agg_unique_strict",
    ] {
        let source = format!(
            "SELECT\n    {function}(\n        item.key,\n        item.value\n    )\nFROM sample_rows item;"
        );
        let expected = format!(
            "SELECT\n    {function}(\n        item.key, item.value\n    )\nFROM sample_rows item;"
        );
        assert_sql(&source, &expected);
    }
}

#[test]
fn compact_calls_and_generic_neighbors_keep_their_layout() {
    assert_sql(
        "SELECT jsonb_build_object('id', 1, 'title', 'sample');",
        "SELECT jsonb_build_object('id', 1, 'title', 'sample');",
    );
    for function in [
        "sample_fn",
        "sample_schema.jsonb_build_object",
        "\"JSON_BUILD_OBJECT\"",
        "json_build_array",
    ] {
        let source = format!(
            "SELECT\n    {function}(\n        'id',\n        1,\n        'title',\n        'sample'\n    );"
        );
        assert_sql(&source, &source);
    }
    let odd =
        "SELECT\n    jsonb_build_object(\n        'id',\n        1,\n        'unpaired'\n    );";
    assert_sql(odd, odd);
    let variadic = "SELECT\n    jsonb_build_object(\n        VARIADIC ARRAY['id', '1']\n    );";
    reviewed(variadic, &FormatOptions::default());
}

#[test]
fn comments_and_blank_boundaries_are_not_crossed() {
    let source = "SELECT\n    jsonb_build_object(\n        'id', -- key note\n        1,\n\n        'title',\n\n        'sample',\n        -- pair note\n        'active',\n        TRUE -- value note\n    );";
    let output = reviewed(source, &FormatOptions::default());
    assert!(output.contains("'id', -- key note\n        1"), "{output}");
    assert!(output.contains("'title',\n\n        'sample'"), "{output}");
    assert!(output.contains("'active', TRUE -- value note"), "{output}");
}

#[test]
fn nested_values_keep_the_key_with_the_value_opening() {
    let source = "SELECT\n    jsonb_build_object(\n        'child',\n        json_build_object(\n            'id',\n            1,\n            'active',\n            TRUE\n        ),\n        'title',\n        'sample'\n    );";
    let expected = "SELECT\n    jsonb_build_object(\n        'child', json_build_object(\n            'id', 1,\n            'active', TRUE\n        ),\n        'title', 'sample'\n    );";
    assert_sql(source, expected);
}

#[test]
fn pairs_use_hard_width_and_split_only_when_needed() {
    let options = FormatOptions {
        soft_line_width: 40,
        hard_line_width: 70,
        ..FormatOptions::default()
    };
    let output = reviewed(
        "SELECT jsonb_build_object('first_label', first_value_expression, 'second_label', second_value_expression, 'third_label', third_value_expression);",
        &options,
    );
    assert!(
        output.contains("'first_label', first_value_expression,"),
        "{output}"
    );
    assert!(
        output.lines().all(|line| line.chars().count() <= 70),
        "{output}"
    );
    let output = reviewed(
        "SELECT\n    jsonb_build_object(\n        'very_long_label_for_this_value',\n        another_long_value_expression_name,\n        'short',\n        1\n    );",
        &options,
    );
    assert!(
        output.contains("'very_long_label_for_this_value',\n"),
        "{output}"
    );
    assert!(output.contains("'short', 1"), "{output}");
}

#[test]
fn aggregate_ordering_and_filter_keep_their_owners() {
    let source = "SELECT\n    jsonb_object_agg(\n        item.key,\n        item.value\n        ORDER BY item.rank, item.id\n    ) FILTER (WHERE item.active)\nFROM sample_rows item;";
    let output = reviewed(source, &FormatOptions::default());
    assert!(output.contains("item.key, item.value\n"), "{output}");
    assert!(output.contains("ORDER BY item.rank, item.id"), "{output}");
    assert!(output.contains("FILTER (WHERE item.active)"), "{output}");
}

#[test]
fn aggregate_order_prefix_is_included_in_the_width_budget() {
    let options = FormatOptions {
        soft_line_width: 68,
        hard_line_width: 68,
        ..FormatOptions::default()
    };
    let output = reviewed(
        "SELECT\n    jsonb_object_agg(\n        item.key,\n        item.value ORDER BY item.first_ordering_column, item.second_ordering_column\n    )\nFROM sample_rows item;",
        &options,
    );
    assert!(
        output.lines().all(|line| line.chars().count() <= 68),
        "{output}"
    );
}

#[test]
fn authored_groups_between_pairs_and_duplicate_keys_are_preserved() {
    let source = "SELECT\n    jsonb_build_object(\n        'id', 1, 'id', 2,\n\n        'label', 'sample'\n    );";
    assert_sql(source, source);
    let source = "SELECT\n    json_build_object(\n        item.key,\n        item.value,\n        'other',\n        2\n    )\nFROM sample_rows item;";
    let output = reviewed(source, &FormatOptions::default());
    assert!(output.contains("item.key, item.value,"), "{output}");
}

#[test]
fn aggregate_distinct_window_and_comment_boundaries_are_preserved() {
    let source = "SELECT\n    pg_catalog.jsonb_object_agg(\n        DISTINCT item.key,\n        item.value\n        ORDER /* ordering */ BY item.key, item.value\n    ) OVER (PARTITION BY item.group_id)\nFROM sample_rows item;";
    let output = reviewed(source, &FormatOptions::default());
    assert!(output.contains("DISTINCT item.key, item.value"), "{output}");
    assert!(output.contains("ORDER /* ordering */ BY"), "{output}");
    assert!(
        output.contains("OVER (PARTITION BY item.group_id)"),
        "{output}"
    );
    reviewed(
        "SELECT jsonb_object_agg(item.key, item.value ORDER BY item.rank, -- rank note\nitem.id) FROM sample_rows item;",
        &FormatOptions::default(),
    );
}

#[test]
fn nested_case_array_and_query_values_remain_safe() {
    for value in [
        "CASE\nWHEN item.active THEN 1\nELSE 0\nEND",
        "ARRAY[\n1,\n2\n]",
        "(SELECT COUNT(*) FROM sample_rows)",
        "COALESCE(\nitem.value,\n'sample'\n)",
    ] {
        let source = format!(
            "SELECT\n    jsonb_build_object(\n        'value',\n        {value},\n        'active',\n        TRUE\n    )\nFROM sample_rows item;"
        );
        let output = reviewed(&source, &FormatOptions::default());
        assert!(output.contains("'active', TRUE"), "{output}");
    }
}

#[test]
fn named_and_explicit_variadic_forms_keep_ordinary_arguments() {
    let source = "SELECT\n    jsonb_build_object(\n        first_arg => 'id',\n        second_arg => 1\n    );";
    assert_sql(source, source);
    let source = "SELECT\n    jsonb_build_object(\n        VARIADIC ARRAY['id', '1', 'title', 'sample']\n    );";
    assert_sql(source, source);
}

#[test]
fn a_leading_comment_does_not_consume_the_pair_line_width() {
    let source = format!(
        "SELECT\n    jsonb_build_object(\n        -- {}\n        'id',\n        1\n    );",
        "note ".repeat(40)
    );
    let output = reviewed(&source, &FormatOptions::default());
    assert!(output.contains("'id', 1"), "{output}");
}
