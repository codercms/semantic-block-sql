mod support;

use support::{assert_sql_layout_only, assert_unsupported};

#[test]
fn formats_sql_standard_routine_with_default_argument() {
    let source = include_str!("fixtures/sql_standard_routine/function.input.sql");
    let expected = include_str!("fixtures/sql_standard_routine/function.expected.sql");
    assert_sql_layout_only(source, expected);
}

#[test]
fn preserves_unreviewed_multi_statement_sql_routines() {
    assert_unsupported(
        "CREATE FUNCTION f()\nRETURNS void\nLANGUAGE SQL\nBEGIN ATOMIC\nSELECT 1;\nSELECT 2;\nEND;",
    );
}

#[test]
fn preserves_language_identifiers_in_sql_standard_routine_signatures() {
    assert_sql_layout_only(
        "CREATE FUNCTION echo_language(language language)\nRETURNS language\nLANGUAGE SQL\nBEGIN ATOMIC\n    SELECT language;\nEND;",
        "CREATE FUNCTION echo_language(language language)\nRETURNS language\nLANGUAGE SQL\nBEGIN ATOMIC\n    SELECT language;\nEND;",
    );
}
