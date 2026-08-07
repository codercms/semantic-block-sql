mod support;

use semblock::{FormatOptions, check_sql};
use support::assert_sql_with as assert_format;

#[test]
fn compact_do_nothing_stays_inline() {
    assert_format(
        "insert into items (id) values ($1) on conflict (id) do nothing returning id;",
        "INSERT INTO items (id) VALUES ($1) ON CONFLICT (id) DO NOTHING RETURNING id;",
        &FormatOptions::default(),
    );
}

#[test]
fn separates_conflict_target_update_set_and_action_where() {
    let source = "insert into items (id, title, updated_at) values ($1, $2, now()) on conflict (id) do update set title=excluded.title, updated_at=now() where items.deleted_at is null and excluded.title is not null returning id;";
    let expected = "INSERT INTO items (id, title, updated_at)\nVALUES ($1, $2, NOW())\nON CONFLICT (id)\nDO UPDATE\nSET\n    title = EXCLUDED.title,\n    updated_at = NOW()\nWHERE items.deleted_at IS NULL AND EXCLUDED.title IS NOT NULL\nRETURNING id;";

    assert_format(source, expected, &FormatOptions::default());

    let checked = check_sql(source, &FormatOptions::default());
    assert!(
        checked
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.rule_id == "layout.on_conflict"),
        "unformatted ON CONFLICT layout should receive a rule-level diagnostic: {:?}",
        checked.diagnostics
    );
}

#[test]
fn complex_target_predicate_remains_owned_by_on_conflict() {
    let source = "insert into items (kp_id, source, title) values ($1, $2, $3) on conflict (kp_id) where kp_id is not null and source = 'kp' do update set title = excluded.title;";
    let expected = "INSERT INTO items (kp_id, source, title)\nVALUES ($1, $2, $3)\nON CONFLICT (kp_id) WHERE kp_id IS NOT NULL AND source = 'kp'\nDO UPDATE\nSET\n    title = EXCLUDED.title;";

    assert_format(source, expected, &FormatOptions::default());
}

#[test]
fn supports_named_constraint_targets() {
    assert_format(
        "insert into items (id, title) values ($1, $2) on conflict on constraint items_pkey do update set title = excluded.title;",
        "INSERT INTO items (id, title)\nVALUES ($1, $2)\nON CONFLICT ON CONSTRAINT items_pkey\nDO UPDATE\nSET\n    title = EXCLUDED.title;",
        &FormatOptions::default(),
    );
}

#[test]
fn on_conflict_comments_remain_attached() {
    let source = "insert into items (id, title, updated_at) values ($1, $2, now()) on conflict (id) do update set title = excluded.title, -- chosen title\nupdated_at = now() returning id;\n";
    let expected = "INSERT INTO items (id, title, updated_at)\nVALUES ($1, $2, NOW())\nON CONFLICT (id)\nDO UPDATE\nSET\n    title = EXCLUDED.title, -- chosen title\n    updated_at = NOW()\nRETURNING id;\n";

    assert_format(source, expected, &FormatOptions::default());
}

#[test]
fn excluded_is_normalized_only_in_the_do_update_action() {
    assert_format(
        "select excluded.title from public.items excluded;",
        "SELECT excluded.title FROM public.items excluded;",
        &FormatOptions::default(),
    );
    assert_format(
        "insert into items as excluded (id) values ($1) on conflict do nothing returning excluded.id;",
        "INSERT INTO items AS excluded (id) VALUES ($1) ON CONFLICT DO NOTHING RETURNING excluded.id;",
        &FormatOptions::default(),
    );
}
