use std::collections::BTreeSet;

use semblock::{FormatOptions, NotEqualPolicy, Severity, check_sql, format_sql_result};

#[test]
fn check_reports_rule_level_style_diagnostics_with_source_ranges() {
    let source = "select count(id),DATE_TRUNC ('day',created_at) from items; \t\n";
    let checked = check_sql(source, &FormatOptions::default());

    assert!(!checked.compliant);
    let rules = checked
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.rule_id.as_str())
        .collect::<BTreeSet<_>>();

    for expected in [
        "casing.keyword",
        "casing.builtin",
        "casing.function",
        "spacing.comma",
        "spacing.function_call",
        "spacing.trailing_whitespace",
    ] {
        assert!(rules.contains(expected), "missing {expected}: {checked:#?}");
    }
    assert!(checked.diagnostics.iter().all(|diagnostic| {
        diagnostic.source_range.start <= diagnostic.source_range.end
            && diagnostic.source_range.end <= source.len()
            && diagnostic.fix_available
            && diagnostic.severity == Severity::Error
    }));
}

#[test]
fn formatted_output_is_clean_and_default_not_equal_is_not_reported() {
    let source = "select count(*) from items where status <> 'deleted';";
    let formatted = format_sql_result(source, &FormatOptions::default());

    assert!(formatted.changed);
    assert_eq!(
        formatted.output,
        "SELECT COUNT(*) FROM items WHERE status <> 'deleted';"
    );
    assert!(!formatted.diagnostics.is_empty());

    let checked = check_sql(&formatted.output, &FormatOptions::default());
    assert!(checked.compliant, "{checked:#?}");
    assert!(checked.diagnostics.is_empty(), "{checked:#?}");
}

#[test]
fn configured_not_equal_and_semicolon_policies_have_dedicated_rules() {
    let options = FormatOptions {
        not_equal_policy: NotEqualPolicy::PreferBang,
        semicolon_policy: semblock::SemicolonPolicy::Require,
        ..FormatOptions::default()
    };
    let checked = check_sql("SELECT 1 <> 2", &options);
    let rules = checked
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.rule_id.as_str())
        .collect::<BTreeSet<_>>();

    assert!(!checked.compliant);
    assert!(rules.contains("operator.not_equal"), "{checked:#?}");
    assert!(rules.contains("statement.semicolon"), "{checked:#?}");
}

#[test]
fn parse_failure_returns_the_original_source_and_syntax_diagnostic() {
    let source = "SELECT FROM;";
    let formatted = format_sql_result(source, &FormatOptions::default());

    assert_eq!(formatted.output, source);
    assert!(!formatted.changed);
    assert_eq!(formatted.diagnostics.len(), 1);
    assert_eq!(formatted.diagnostics[0].rule_id, "syntax.parse_failure");
    assert_eq!(formatted.diagnostics[0].severity, Severity::Error);
    assert!(!formatted.diagnostics[0].fix_available);

    let checked = check_sql(source, &FormatOptions::default());
    assert!(!checked.compliant);
    assert_eq!(checked.diagnostics, formatted.diagnostics);
}

#[test]
fn allowed_indivisible_over_hard_token_is_only_a_warning() {
    let source = "SELECT 'this literal is intentionally longer than the configured hard width';";
    let options = FormatOptions {
        soft_line_width: 32,
        hard_line_width: 40,
        ..FormatOptions::default()
    };
    let checked = check_sql(source, &options);

    assert!(checked.compliant, "{checked:#?}");
    assert_eq!(checked.diagnostics.len(), 1);
    assert_eq!(checked.diagnostics[0].rule_id, "layout.hard_line_width");
    assert_eq!(checked.diagnostics[0].severity, Severity::Warning);
    assert!(!checked.diagnostics[0].fix_available);
}

#[test]
fn tokenless_whitespace_changes_still_have_a_rule_level_diagnostic() {
    let source = " \t\n";
    let checked = check_sql(source, &FormatOptions::default());

    assert!(!checked.compliant);
    assert_eq!(checked.diagnostics.len(), 1);
    assert_eq!(
        checked.diagnostics[0].rule_id,
        "spacing.trailing_whitespace"
    );
    assert_eq!(checked.diagnostics[0].source_range.start, 0);
    assert_eq!(checked.diagnostics[0].source_range.end, source.len());
}
