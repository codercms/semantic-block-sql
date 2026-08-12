use pretty_assertions::assert_eq;
use semblock::config::GoConfig;
use semblock::source::{Language, format_source};
use semblock::{
    FormatOptions, FormatWarning, Severity, check_sql, format_sql, validate_equivalent,
};

fn format_fixture(name: &str, options: &FormatOptions) -> String {
    let input = match name {
        "authored-groups" => include_str!("fixtures/batch2/authored-groups.input.sql"),
        "case" => include_str!("fixtures/batch2/case.input.sql"),
        "comments" => include_str!("fixtures/batch2/comments.input.sql"),
        "cte" => include_str!("fixtures/batch2/cte.input.sql"),
        "function-arguments" => include_str!("fixtures/batch2/function-arguments.input.sql"),
        "recursive-cte" => include_str!("fixtures/batch2/recursive-cte.input.sql"),
        "width-packing" => include_str!("fixtures/batch2/width-packing.input.sql"),
        _ => panic!("unknown fixture"),
    };

    let formatted = format_sql(input, options).expect("format succeeds");
    validate_equivalent(input, &formatted.output).expect("formatting is semantically equivalent");
    assert_eq!(
        format_sql(&formatted.output, options)
            .expect("second format succeeds")
            .output,
        formatted.output,
        "fixture must be idempotent"
    );
    formatted.output
}

#[test]
fn preserves_authored_list_groups_blank_lines_and_comment_boundaries() {
    assert_eq!(
        format_fixture("authored-groups", &FormatOptions::default()),
        include_str!("fixtures/batch2/authored-groups.expected.sql")
    );
}

#[test]
fn expands_one_line_lists_to_one_item_per_line_and_obeys_hard_width() {
    let options = FormatOptions {
        soft_line_width: 52,
        hard_line_width: 68,
        ..FormatOptions::default()
    };
    let output = format_fixture("width-packing", &options);
    assert_eq!(
        output,
        include_str!("fixtures/batch2/width-packing.expected.sql")
    );
    assert!(
        output.lines().all(|line| line.chars().count() <= 68),
        "all breakable lines stay within the hard limit"
    );
    assert!(output.contains("    item.id,\n    item.kp_identifier,"));
}

#[test]
fn cohesive_authored_group_may_exceed_soft_but_is_split_at_hard_width() {
    let source = "\
SELECT
    item.first_identifier, item.second_identifier,
    item.third_identifier, item.fourth_identifier, item.fifth_identifier
FROM public.items item;
";
    let options = FormatOptions {
        soft_line_width: 48,
        hard_line_width: 72,
        ..FormatOptions::default()
    };

    let output = format_sql(source, &options)
        .expect("format succeeds")
        .output;
    assert!(output.contains("item.first_identifier, item.second_identifier,"));
    assert!(
        output
            .lines()
            .all(|line| line.chars().count() <= options.hard_line_width)
    );
}

#[test]
fn formats_compact_and_expanded_case_without_branch_indentation_storms() {
    assert_eq!(
        format_fixture("case", &FormatOptions::default()),
        include_str!("fixtures/batch2/case.expected.sql")
    );
}

#[test]
fn formats_multiple_ctes_as_nested_query_blocks() {
    assert_eq!(
        format_fixture("cte", &FormatOptions::default()),
        include_str!("fixtures/batch2/cte.expected.sql")
    );
}

#[test]
fn packs_function_arguments_without_inventing_business_groups() {
    assert_eq!(
        format_fixture("function-arguments", &FormatOptions::default()),
        include_str!("fixtures/batch2/function-arguments.expected.sql")
    );
}

#[test]
fn function_argument_lines_obey_the_hard_width() {
    let options = FormatOptions {
        soft_line_width: 52,
        hard_line_width: 68,
        ..FormatOptions::default()
    };
    let output = format_fixture("function-arguments", &options);
    assert!(
        output.lines().all(|line| line.chars().count() <= 68),
        "function arguments break only at comma boundaries"
    );
}

#[test]
fn separates_recursive_cte_anchor_and_recursive_terms() {
    assert_eq!(
        format_fixture("recursive-cte", &FormatOptions::default()),
        include_str!("fixtures/batch2/recursive-cte.expected.sql")
    );
}

#[test]
fn authored_list_groups_are_mandatory() {
    let source = "\
SELECT
    item.id,
    item.kp_id,
    item.imdb_id
FROM public.items item;
";
    let output = format_sql(source, &FormatOptions::default())
        .expect("format succeeds")
        .output;

    assert_eq!(output, source);
}

#[test]
fn blank_lines_and_comment_boundaries_are_mandatory() {
    let output = format_fixture("authored-groups", &FormatOptions::default());

    assert!(output.contains("item.title_orig,\n\n"));
    assert!(output.contains("item.title_orig,\n\n    -- audit fields"));
}

#[test]
fn expands_a_join_predicate_when_width_hides_its_structure() {
    let options = FormatOptions {
        soft_line_width: 72,
        ..FormatOptions::default()
    };
    let source = "select item.id from public.items item left join match_new.source_links link on link.very_long_external_identifier = item.very_long_external_identifier;";
    let expected = "\
SELECT item.id
FROM public.items item
LEFT JOIN match_new.source_links link ON
    link.very_long_external_identifier = item.very_long_external_identifier;";

    assert_eq!(
        format_sql(source, &options)
            .expect("format succeeds")
            .output,
        expected
    );
}

#[test]
fn preserves_leading_trailing_and_list_comments_byte_for_byte() {
    let source = include_str!("fixtures/batch2/comments.input.sql");
    let output = format_fixture("comments", &FormatOptions::default());
    assert_eq!(
        output,
        include_str!("fixtures/batch2/comments.expected.sql")
    );
    validate_equivalent(source, &output).expect("comment attachment and contents remain stable");
}

#[test]
fn normalizes_comment_trailing_whitespace_without_skipping_the_statement() {
    let source = "WITH existing AS (\r\n    -- Compare prices\u{a0}\t \r\n    SELECT id FROM offers\r\n)\r\nSELECT id FROM existing;\r\n";
    let expected = "WITH existing AS (\n    -- Compare prices\n    SELECT id\n    FROM offers\n)\nSELECT id\nFROM existing;\n";
    let trailing_space = source
        .find("\u{a0}\t \r\n    SELECT")
        .expect("comment trailing whitespace");
    let trailing_end = trailing_space + "\u{a0}\t ".len();

    let formatted = format_sql(source, &FormatOptions::default()).expect("format succeeds");

    assert_eq!(formatted.output, expected);
    assert!(
        !formatted
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.rule_id == "format.statement_skipped")
    );
    assert!(formatted.diagnostics.iter().any(|diagnostic| {
        diagnostic.rule_id == "spacing.trailing_whitespace"
            && diagnostic.severity == Severity::Error
            && diagnostic.source_range.start == trailing_space
            && diagnostic.source_range.end == trailing_end
            && diagnostic.fix_available
    }));
    assert_eq!(
        format_sql(expected, &FormatOptions::default())
            .expect("normalized SQL is idempotent")
            .output,
        expected
    );
    assert!(check_sql(expected, &FormatOptions::default()).compliant);

    let source_result = format_source(
        source,
        Language::Sql,
        &FormatOptions::default(),
        &GoConfig::default(),
    )
    .expect("source facade preserves CRLF");
    assert_eq!(source_result.output, expected.replace('\n', "\r\n"));
    assert!(source_result.diagnostics.iter().any(|diagnostic| {
        diagnostic.rule_id == "spacing.trailing_whitespace"
            && diagnostic.source_range.start == trailing_space
            && diagnostic.source_range.end == trailing_end
    }));
}

#[test]
fn normalizes_trailing_whitespace_across_comment_forms() {
    for (name, source, expected, removed_ranges) in [
        (
            "inline line comment",
            "SELECT 1; -- inline\u{2003}\t \n",
            "SELECT 1; -- inline\n",
            1,
        ),
        (
            "line comment at EOF",
            "SELECT 1; -- eof\u{a0}\t ",
            "SELECT 1; -- eof",
            1,
        ),
        (
            "multiline block comment",
            "SELECT /* first\u{2003}\nsecond\u{3000}\r\nthird */ 1;",
            "SELECT /* first\nsecond\r\nthird */ 1;",
            2,
        ),
    ] {
        let formatted = format_sql(source, &FormatOptions::default())
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(formatted.output, expected, "{name}");
        assert_eq!(
            formatted
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.rule_id == "spacing.trailing_whitespace")
                .count(),
            removed_ranges,
            "{name}"
        );
        assert!(
            !formatted
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.rule_id == "format.statement_skipped")
        );
        validate_equivalent(source, expected).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            format_sql(expected, &FormatOptions::default())
                .unwrap_or_else(|error| panic!("{name}: {error}"))
                .output,
            expected,
            "{name}: idempotence"
        );
    }
}

#[test]
fn preserves_non_trailing_comment_and_protected_token_whitespace() {
    let source = "SELECT 'literal \t', \"quoted name\", $$body \t$$; -- keep  internal and zero-width\u{200b}\n";
    let formatted = format_sql(source, &FormatOptions::default()).expect("format succeeds");

    assert_eq!(formatted.output, source);
    assert!(formatted.diagnostics.is_empty(), "{formatted:#?}");
    validate_equivalent(source, &formatted.output).expect("protected bytes remain exact");
}

#[test]
fn reports_indivisible_tokens_that_make_a_line_exceed_the_hard_width() {
    let source =
        "SELECT 'this literal is intentionally much longer than the configured hard width';";
    let options = FormatOptions {
        soft_line_width: 32,
        hard_line_width: 40,
        ..FormatOptions::default()
    };

    let formatted = format_sql(source, &options).expect("indivisible token is allowed");
    assert_eq!(formatted.output, source);
    assert_eq!(
        formatted.warnings,
        vec![FormatWarning::IndivisibleTokenExceedsHardWidth { line: 1, width: 82 }]
    );
}

#[test]
fn indivisible_width_diagnostics_point_to_the_source_line() {
    let literal = "'this literal is intentionally much longer than the configured hard width'";
    let source = format!("CREATE TABLE sample (id bigint);\nSELECT {literal};");
    let options = FormatOptions {
        soft_line_width: 32,
        hard_line_width: 40,
        ..FormatOptions::default()
    };

    let formatted = format_sql(&source, &options).expect("indivisible token is allowed");
    let diagnostic = formatted
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.rule_id == "layout.hard_line_width")
        .expect("hard-width warning");

    assert_eq!(
        &source[diagnostic.source_range.start..diagnostic.source_range.end],
        literal
    );
    assert!(
        diagnostic.message.contains("source line 2"),
        "{}",
        diagnostic.message
    );
    assert!(
        !diagnostic.message.contains("line 4"),
        "{}",
        diagnostic.message
    );
}

#[test]
fn four_space_indentation_is_mandatory_for_real_syntax_nesting() {
    let output = format_sql(
        "select item.id from public.items item where item.deleted_at is null and (item.title_rus is not null or item.title_orig is not null);",
        &FormatOptions::default(),
    )
    .expect("format succeeds")
    .output;

    assert!(output.contains("\n    item.deleted_at IS NULL"));
    assert!(output.contains("\n        item.title_rus IS NOT NULL"));
}

#[test]
fn quoted_function_and_type_identifiers_are_never_case_normalized() {
    let source = "select \"MyFunc\"(item.id), item.value::\"MyType\" from public.items item;";
    let output = format_sql(source, &FormatOptions::default())
        .expect("format succeeds")
        .output;

    assert!(output.contains("\"MyFunc\"(item.id)"));
    assert!(output.contains("item.value::\"MyType\""));
}

#[test]
fn inline_block_comment_keeps_a_lexical_separator() {
    let source = "/* injected */ select item.id from public.items item;";
    let output = format_sql(source, &FormatOptions::default())
        .expect("format succeeds")
        .output;

    assert_eq!(
        output,
        "/* injected */ SELECT item.id FROM public.items item;"
    );
    validate_equivalent(source, &output).expect("comment attachment remains equivalent");
}
