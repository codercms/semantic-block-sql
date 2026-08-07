mod support;

use support::assert_fixture_pair;

#[test]
fn owns_set_operations_inside_ctes_and_derived_sources() {
    assert_fixture_pair("architecture_tightening", "set-operations");
}

#[test]
fn preserves_short_authored_groups_through_one_policy() {
    assert_fixture_pair("architecture_tightening", "compact-groups");
}

#[test]
fn attaches_standalone_comments_to_the_following_boolean_branch() {
    assert_fixture_pair("architecture_tightening", "comment-trivia");
}
