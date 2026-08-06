use pretty_assertions::assert_eq;
use semblock::{FormatOptions, check_sql, format_sql, validate_equivalent};

fn assert_fixture(name: &str) {
    let input = std::fs::read_to_string(format!(
        "tests/fixtures/architecture_tightening/{name}.input.sql"
    ))
    .expect("fixture input");
    let expected = std::fs::read_to_string(format!(
        "tests/fixtures/architecture_tightening/{name}.expected.sql"
    ))
    .expect("fixture expectation");
    let options = FormatOptions::default();

    let formatted = format_sql(&input, &options).expect("format succeeds");
    assert_eq!(formatted.output, expected);
    validate_equivalent(&input, &expected).expect("semantic equivalence");
    assert_eq!(
        format_sql(&expected, &options)
            .expect("second format succeeds")
            .output,
        expected,
        "formatting must be idempotent",
    );
    assert!(
        check_sql(&expected, &options).compliant,
        "expected fixture must be compliant",
    );
}

#[test]
fn owns_set_operations_inside_ctes_and_derived_sources() {
    assert_fixture("set-operations");
}

#[test]
fn preserves_short_authored_groups_through_one_policy() {
    assert_fixture("compact-groups");
}

#[test]
fn attaches_standalone_comments_to_the_following_boolean_branch() {
    assert_fixture("comment-trivia");
}
