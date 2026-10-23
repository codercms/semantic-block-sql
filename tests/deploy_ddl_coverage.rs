mod support;

#[test]
fn lock_table_modes_have_explicit_golden_layout() {
    for mode in [
        "access share",
        "row share",
        "row exclusive",
        "share update exclusive",
        "share",
        "share row exclusive",
        "exclusive",
        "access exclusive",
    ] {
        for nowait in ["", " nowait"] {
            support::assert_sql(
                &format!("lock table demo.samples in {mode} mode{nowait};"),
                &format!(
                    "LOCK TABLE demo.samples IN {} MODE{};",
                    mode.to_uppercase(),
                    nowait.to_uppercase()
                ),
            );
        }
    }
    support::assert_sql("lock demo.samples;", "LOCK demo.samples;");
    support::assert_sql(
        "lock table only demo.samples, demo.measurements in share mode;",
        "LOCK TABLE ONLY demo.samples, demo.measurements IN SHARE MODE;",
    );
}

#[test]
fn table_and_constraint_renames_preserve_identifiers() {
    for (source, expected) in [
        (
            "alter table demo.samples rename to observations;",
            "ALTER TABLE demo.samples RENAME TO observations;",
        ),
        (
            "alter table if exists only demo.samples rename constraint samples_pk to observations_pk;",
            "ALTER TABLE IF EXISTS ONLY demo.samples RENAME CONSTRAINT samples_pk TO observations_pk;",
        ),
        (
            "alter table demo.samples rename to comment;",
            "ALTER TABLE demo.samples RENAME TO comment;",
        ),
        (
            "alter table demo.samples rename constraint comment to options;",
            "ALTER TABLE demo.samples RENAME CONSTRAINT comment TO options;",
        ),
        (
            "alter table \"Mixed Table\" rename to \"New Table\";",
            "ALTER TABLE \"Mixed Table\" RENAME TO \"New Table\";",
        ),
    ] {
        support::assert_sql(source, expected);
    }
}

#[test]
fn alter_column_type_and_using_have_their_own_ast_shape() {
    for (source, expected) in [
        (
            "alter table demo.samples alter column label type text using label::text;",
            "ALTER TABLE demo.samples ALTER COLUMN label TYPE text USING label::text;",
        ),
        (
            "alter table demo.samples alter label set data type varchar(40);",
            "ALTER TABLE demo.samples ALTER label SET DATA TYPE varchar(40);",
        ),
        (
            "alter table demo.samples alter column label type text collate \"C\" using trim(label);",
            "ALTER TABLE demo.samples ALTER COLUMN label TYPE text COLLATE \"C\" USING trim(label);",
        ),
    ] {
        support::assert_sql(source, expected);
    }
}

#[test]
fn static_ddl_inside_a_procedural_block_uses_the_sql_formatter() {
    support::assert_sql_layout_only(
        "DO $$ BEGIN IF TRUE THEN ALTER TABLE demo.samples ALTER COLUMN payload SET COMPRESSION lz4; END IF; END; $$;",
        "DO $$\nBEGIN\n    IF TRUE THEN\n        ALTER TABLE demo.samples ALTER COLUMN payload SET COMPRESSION lz4;\n    END IF;\nEND;\n$$;",
    );
}

#[test]
fn analyze_and_analyse_are_both_owned_spellings() {
    for spelling in ["analyze", "analyse"] {
        for (source, expected) in [
            (
                format!("{spelling} demo.samples;"),
                format!("{} demo.samples;", spelling.to_uppercase()),
            ),
            (
                format!("{spelling} verbose demo.samples (label, score);"),
                format!(
                    "{} VERBOSE demo.samples(label, score);",
                    spelling.to_uppercase()
                ),
            ),
            (
                format!("vacuum {spelling} demo.samples;"),
                format!("VACUUM {} demo.samples;", spelling.to_uppercase()),
            ),
            (
                format!("vacuum ({spelling}) demo.samples;"),
                format!("VACUUM ({}) demo.samples;", spelling.to_uppercase()),
            ),
        ] {
            support::assert_sql(&source, &expected);
        }
    }
}

#[test]
fn lock_lists_preserve_groups_comments_and_keyword_identifiers() {
    support::assert_sql(
        "lock table\n demo.samples, demo.observations,\n demo.readings\n in share mode nowait;",
        "LOCK TABLE\n    demo.samples, demo.observations,\n    demo.readings\nIN SHARE MODE NOWAIT;",
    );
    support::assert_sql(
        "lock table demo.samples, -- coordinate readers\n demo.observations in row share mode;",
        "LOCK TABLE\n    demo.samples, -- coordinate readers\n    demo.observations\nIN ROW SHARE MODE;",
    );
    for (source, expected) in [
        ("lock table nowait;", "LOCK TABLE nowait;"),
        (
            "lock table access, mode nowait;",
            "LOCK TABLE access, mode NOWAIT;",
        ),
        (
            "lock table \"Mixed Schema\".\"Readings\" * in exclusive mode;",
            "LOCK TABLE \"Mixed Schema\".\"Readings\" * IN EXCLUSIVE MODE;",
        ),
    ] {
        support::assert_sql(source, expected);
    }
    let options = semblock::FormatOptions {
        soft_line_width: 44,
        hard_line_width: 70,
        ..Default::default()
    };
    support::assert_sql_with(
        "lock table demo.temperature_observations, demo.pressure_observations in access exclusive mode nowait;",
        "LOCK TABLE\n    demo.temperature_observations,\n    demo.pressure_observations\nIN ACCESS EXCLUSIVE MODE NOWAIT;",
        &options,
    );
}

#[test]
fn type_changes_cover_arrays_modifiers_qualified_types_and_multiple_actions() {
    for (source, expected) in [
        (
            "alter table demo.samples alter column score type numeric(12, 3) using score::numeric(12, 3);",
            "ALTER TABLE demo.samples ALTER COLUMN score TYPE numeric(12, 3) USING score::numeric(12, 3);",
        ),
        (
            "alter table demo.samples alter column labels set data type text[] using array[label];",
            "ALTER TABLE demo.samples ALTER COLUMN labels SET DATA TYPE text[] USING ARRAY[label];",
        ),
        (
            "alter table demo.samples alter column comment type demo.\"Label\" using comment::demo.\"Label\";",
            "ALTER TABLE demo.samples ALTER COLUMN comment TYPE demo.\"Label\" USING comment::demo.\"Label\";",
        ),
        (
            "alter table demo.samples alter column score type bigint, alter column label set not null;",
            "ALTER TABLE demo.samples\n    ALTER COLUMN score TYPE bigint,\n    ALTER COLUMN label SET NOT NULL;",
        ),
    ] {
        support::assert_sql(source, expected);
    }
}

#[test]
fn procedural_ddl_leaves_use_the_same_reviewed_capabilities() {
    for compression in ["lz4", "pglz", "default"] {
        let expected_compression = if compression == "default" {
            "DEFAULT"
        } else {
            compression
        };
        support::assert_sql_layout_only(
            &format!(
                "do $$ begin alter table demo.samples alter column payload set compression {compression}; end $$;"
            ),
            &format!(
                "DO $$\nBEGIN\n    ALTER TABLE demo.samples ALTER COLUMN payload SET COMPRESSION {expected_compression};\nEND\n$$;"
            ),
        );
    }
    support::assert_sql_layout_only(
        "do $$ begin lock table demo.samples in share mode; alter table demo.samples rename to observations; create table demo.stage (id int); drop table demo.stage; end; $$;",
        "DO $$\nBEGIN\n    LOCK TABLE demo.samples IN SHARE MODE;\n    ALTER TABLE demo.samples RENAME TO observations;\n    CREATE TABLE demo.stage (\n        id int\n    );\n    DROP TABLE demo.stage;\nEND;\n$$;",
    );
}

#[test]
fn neighboring_unreviewed_ddl_remains_byte_identical() {
    support::assert_unsupported_cases(&[
        (
            "column rename",
            "alter table demo.samples rename column old_label to new_label;",
        ),
        (
            "publication",
            "create publication readings_feed for table demo.samples;",
        ),
    ]);
    let source = "do $$ begin create publication readings_feed for table demo.samples; end $$;";
    let result = semblock::format_sql_result(source, &semblock::FormatOptions::default());
    assert!(
        result
            .output
            .contains("create publication readings_feed for table demo.samples;")
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.rule_id == "syntax.unsupported")
    );
    let strict = semblock::FormatOptions {
        unsupported_policy: semblock::UnsupportedPolicy::Error,
        ..Default::default()
    };
    assert_eq!(semblock::format_sql_result(source, &strict).output, source);
}
