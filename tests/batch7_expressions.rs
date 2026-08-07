mod support;

use semblock::FormatOptions;
use support::assert_sql_with as assert_format;

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
fn lowercases_multiword_type_names_without_reclassifying_time_zone_syntax() {
    let source = "select cast(value as TIMESTAMP(3) with TIME ZONE) as timestamp_value, value::DOUBLE PRECISION as double_value, value::CHARACTER VARYING(20) as text_value, created_at at time zone 'UTC' as utc_value;";
    let expected = "SELECT
    CAST(value AS timestamp(3) WITH time zone) AS timestamp_value,
    value::double precision AS double_value,
    value::character varying(20) AS text_value,
    created_at AT TIME ZONE 'UTC' AS utc_value;";

    assert_format(source, expected, &FormatOptions::default());
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
