mod support;

#[test]
fn drop_column_guards_belong_to_actions_not_the_table_header() {
    support::assert_sql(
        "alter table lab.readings drop column if exists obsolete_value, drop column if exists legacy_unit;",
        "ALTER TABLE lab.readings\n    DROP COLUMN IF EXISTS obsolete_value,\n    DROP COLUMN IF EXISTS legacy_unit;",
    );
}

#[test]
fn action_like_table_names_are_not_action_boundaries() {
    for name in [
        "enable", "disable", "reset", "validate", "attach", "detach", "force", "owner", "cluster",
        "no",
    ] {
        support::assert_sql(
            &format!("alter table {name} drop column stale_value, drop column stale_unit;"),
            &format!(
                "ALTER TABLE {name}\n    DROP COLUMN stale_value,\n    DROP COLUMN stale_unit;"
            ),
        );
    }
}

#[test]
fn table_and_action_guards_are_independent_for_columns_and_constraints() {
    for table_guard in ["", " if exists"] {
        for target in [
            "lab.readings",
            "only lab.readings",
            "lab.readings *",
            "only (lab.readings)",
        ] {
            for (action, expected) in [
                (
                    "drop column if exists legacy_value",
                    "DROP COLUMN IF EXISTS legacy_value",
                ),
                (
                    "drop if exists legacy_value cascade",
                    "DROP IF EXISTS legacy_value CASCADE",
                ),
                (
                    "drop constraint if exists readings_legacy_key restrict",
                    "DROP CONSTRAINT IF EXISTS readings_legacy_key RESTRICT",
                ),
                (
                    "alter column sample_id drop identity if exists",
                    "ALTER COLUMN sample_id DROP IDENTITY IF EXISTS",
                ),
                (
                    "alter column computed_value drop expression if exists",
                    "ALTER COLUMN computed_value DROP EXPRESSION IF EXISTS",
                ),
                (
                    "add column if not exists note text",
                    "ADD COLUMN IF NOT EXISTS note text",
                ),
            ] {
                support::assert_sql(
                    &format!("alter table{table_guard} {target} {action};"),
                    &format!(
                        "ALTER TABLE{} {} {expected};",
                        table_guard.to_uppercase(),
                        target.replace("only", "ONLY")
                    ),
                );
            }
        }
    }
}

#[test]
fn quoted_and_qualified_relations_bind_past_the_complete_name() {
    for target in [
        "enable.validate",
        "archive.enable.validate",
        "comment.options.mode",
        "\"Mixed Schema\".\"IF EXISTS\"",
        "lab.\"a.b\"",
        "lab./* qualifier */ readings",
    ] {
        support::assert_sql(
            &format!("alter table {target} drop column legacy_value, drop column legacy_unit;"),
            &format!(
                "ALTER TABLE {target}\n    DROP COLUMN legacy_value,\n    DROP COLUMN legacy_unit;"
            ),
        );
    }
}

#[test]
fn guarded_drop_targets_keep_keyword_like_identifier_spelling() {
    support::assert_sql(
        "alter table lab.readings drop column if exists comment, drop constraint if exists options;",
        "ALTER TABLE lab.readings\n    DROP COLUMN IF EXISTS comment,\n    DROP CONSTRAINT IF EXISTS options;",
    );
}
