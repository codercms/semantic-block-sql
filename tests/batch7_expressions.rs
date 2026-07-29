use pretty_assertions::assert_eq;
use semblock::{FormatOptions, check_sql, format_sql, validate_equivalent};

fn assert_format(source: &str, expected: &str, options: &FormatOptions) {
    let formatted = format_sql(source, options).expect("format succeeds");
    assert_eq!(formatted.output, expected);
    assert!(
        formatted.warnings.is_empty(),
        "warnings: {:?}",
        formatted.warnings
    );
    validate_equivalent(source, expected).expect("semantic equivalence");
    assert_eq!(
        format_sql(expected, options)
            .expect("second format succeeds")
            .output,
        expected,
        "formatting must be idempotent"
    );
    let checked = check_sql(expected, options);
    assert!(
        checked.compliant,
        "formatted SQL must pass check: {:?}",
        checked.diagnostics
    );
}

#[test]
fn formats_casts_array_types_constructors_subscripts_and_slices() {
    assert_format(
        include_str!("fixtures/batch7/casts-arrays.input.sql"),
        include_str!("fixtures/batch7/casts-arrays.expected.sql"),
        &FormatOptions::default(),
    );
}

#[test]
fn preserves_quoted_type_names_and_array_expression_comments() {
    let source = "select value::\"Public\".\"CustomType\"[] as typed_value, -- cast stays quoted\narray[1,2,3] as ids, -- constructor\ntags[1:2] as sample from items;";
    let expected = "SELECT\n    value::\"Public\".\"CustomType\"[] AS typed_value, -- cast stays quoted\n    ARRAY[1, 2, 3] AS ids, -- constructor\n    tags[1:2] AS sample\nFROM items;";

    assert_format(source, expected, &FormatOptions::default());
}

#[test]
fn does_not_treat_an_implicit_alias_after_a_cast_as_part_of_the_type_name() {
    let source = "select value::Public.Custom_Type AliasName from items;";
    let expected = "SELECT value::public.custom_type AliasName FROM items;";

    assert_format(source, expected, &FormatOptions::default());
}

#[test]
fn keeps_array_punctuation_tight_under_narrow_widths_without_changing_aliases() {
    let source = "select array[1,2,3] as Identifiers, tags[1:3] as SelectedTags, json_object('key': tags[1]) as Document from items;";
    let expected = "SELECT
    ARRAY[1, 2, 3] AS Identifiers,
    tags[1:3] AS SelectedTags,
    json_object('key' : tags[1]) AS Document
FROM items;";
    let options = FormatOptions {
        soft_line_width: 48,
        hard_line_width: 72,
        ..FormatOptions::default()
    };

    assert_format(source, expected, &options);
}

#[test]
fn formats_postgresql_json_operators_across_owned_statements() {
    assert_format(
        include_str!("fixtures/batch7/json-operators.input.sql"),
        include_str!("fixtures/batch7/json-operators.expected.sql"),
        &FormatOptions::default(),
    );
}

#[test]
fn preserves_simple_fixture_backed_sql_json_constructors() {
    let source = "select json_object('a': value), json_array(value);";
    let expected = "SELECT json_object('a' : value), json_array(value);";

    assert_format(source, expected, &FormatOptions::default());
}
