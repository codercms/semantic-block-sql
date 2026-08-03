use pretty_assertions::assert_eq;
use semblock::{FormatOptions, check_sql, format_sql, format_sql_result, validate_equivalent};

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
fn formats_create_table_columns_and_constraints() {
    let source = "create table if not exists public.users (id bigint generated always as identity,email text not null,created_at timestamptz default now(),constraint users_pkey primary key(id),unique nulls not distinct(email),check(email <> ''));";
    let expected = "CREATE TABLE IF NOT EXISTS public.users (\n    id bigint GENERATED ALWAYS AS IDENTITY,\n    email text NOT NULL,\n    created_at timestamptz DEFAULT NOW(),\n\n    CONSTRAINT users_pkey PRIMARY KEY (id),\n    UNIQUE NULLS NOT DISTINCT (email),\n    CHECK (email <> '')\n);";

    assert_format(source, expected, &FormatOptions::default());
}

#[test]
fn wraps_create_table_check_predicates_like_where_predicates() {
    let source = "CREATE TABLE queue (\n    status text,\n    claim_token uuid,\n    claimed_revision bigint,\n    processing_started_at timestamptz,\n\n    CHECK ((status = 'processing' AND claim_token IS NOT NULL AND claimed_revision IS NOT NULL AND processing_started_at IS NOT NULL) OR (status != 'processing' AND claim_token IS NULL AND claimed_revision IS NULL AND processing_started_at IS NULL))\n);";
    let expected = "CREATE TABLE queue (\n    status text,\n    claim_token uuid,\n    claimed_revision bigint,\n    processing_started_at timestamptz,\n\n    CHECK (\n        (\n            status = 'processing'\n            AND claim_token IS NOT NULL\n            AND claimed_revision IS NOT NULL\n            AND processing_started_at IS NOT NULL\n        )\n        OR (\n            status != 'processing'\n            AND claim_token IS NULL\n            AND claimed_revision IS NULL\n            AND processing_started_at IS NULL\n        )\n    )\n);";

    let formatted =
        format_sql(source, &FormatOptions::default()).expect("CHECK predicate formatting succeeds");
    assert_eq!(formatted.output, expected);
    assert!(
        formatted.warnings.is_empty(),
        "breakable CHECK predicates are not indivisible: {:?}",
        formatted.warnings
    );
    assert_format(expected, expected, &FormatOptions::default());
}

#[test]
fn wraps_alter_table_check_predicates_like_where_predicates() {
    let source = "ALTER TABLE queue ADD CONSTRAINT queue_claim_check CHECK ((status = 'processing' AND claim_token IS NOT NULL AND claimed_revision IS NOT NULL) OR (status != 'processing' AND claim_token IS NULL AND claimed_revision IS NULL));";
    let expected = "ALTER TABLE queue\n    ADD CONSTRAINT queue_claim_check CHECK (\n        (\n            status = 'processing'\n            AND claim_token IS NOT NULL\n            AND claimed_revision IS NOT NULL\n        )\n        OR (\n            status != 'processing'\n            AND claim_token IS NULL\n            AND claimed_revision IS NULL\n        )\n    );";

    assert_format(source, expected, &FormatOptions::default());
}

#[test]
fn wraps_column_check_predicates_in_create_and_alter_table() {
    assert_format(
        "CREATE TABLE metrics (score integer CHECK (score >= 0 AND score <= 100));",
        "CREATE TABLE metrics (\n    score integer CHECK (score >= 0 AND score <= 100)\n);",
        &FormatOptions::default(),
    );
    assert_format(
        "ALTER TABLE metrics ADD COLUMN weight integer CHECK (weight >= 0 AND weight <= 100);",
        "ALTER TABLE metrics\n    ADD COLUMN weight integer CHECK (weight >= 0 AND weight <= 100);",
        &FormatOptions::default(),
    );
}

#[test]
fn keeps_simple_indexes_compact_and_expands_complex_indexes() {
    assert_format(
        "create index users_reg_date_idx on users (reg_date);",
        "CREATE INDEX users_reg_date_idx ON users (reg_date);",
        &FormatOptions::default(),
    );

    let source = "create index concurrently if not exists users_email_idx on public.users using btree (lower(email) text_pattern_ops desc nulls last) include (id) with (fillfactor=90) tablespace fast where deleted_at is null;";
    let expected = "CREATE INDEX CONCURRENTLY IF NOT EXISTS users_email_idx ON public.users USING btree (lower(email) text_pattern_ops DESC NULLS LAST)\nINCLUDE (id)\nWITH (fillfactor = 90)\nTABLESPACE fast\nWHERE deleted_at IS NULL;";
    assert_format(source, expected, &FormatOptions::default());
}

#[test]
fn formats_alter_table_actions_and_syntactic_groups() {
    let source = "alter table if exists public.users add column name text, alter column email set not null, add constraint users_name_key unique(name), drop column old_name;";
    let expected = "ALTER TABLE IF EXISTS public.users\n    ADD COLUMN name text,\n\n    ALTER COLUMN email SET NOT NULL,\n\n    ADD CONSTRAINT users_name_key UNIQUE (name),\n\n    DROP COLUMN old_name;";

    assert_format(source, expected, &FormatOptions::default());
}

#[test]
fn formats_alter_table_relation_options_as_an_owned_list() {
    let source = "ALTER TABLE i1688.offer_fetch_queue SET (\n    autovacuum_vacuum_scale_factor = 0.01, autovacuum_vacuum_threshold = 1000,\n    autovacuum_analyze_scale_factor = 0.02, autovacuum_analyze_threshold = 1000\n);";
    let expected = "ALTER TABLE i1688.offer_fetch_queue\n    SET (\n        autovacuum_vacuum_scale_factor = 0.01, autovacuum_vacuum_threshold = 1000,\n        autovacuum_analyze_scale_factor = 0.02, autovacuum_analyze_threshold = 1000\n    );";
    assert_format(source, expected, &FormatOptions::default());

    let ungrouped = "ALTER TABLE i1688.offer_fetch_queue SET (autovacuum_vacuum_scale_factor=0.01,autovacuum_vacuum_threshold=1000,autovacuum_analyze_scale_factor=0.02,autovacuum_analyze_threshold=1000);";
    let expanded = "ALTER TABLE i1688.offer_fetch_queue\n    SET (\n        autovacuum_vacuum_scale_factor = 0.01,\n        autovacuum_vacuum_threshold = 1000,\n        autovacuum_analyze_scale_factor = 0.02,\n        autovacuum_analyze_threshold = 1000\n    );";
    assert_format(ungrouped, expanded, &FormatOptions::default());

    assert_format(
        "ALTER TABLE public.items RESET (\n    fillfactor, autovacuum_vacuum_threshold\n);",
        "ALTER TABLE public.items\n    RESET (\n        fillfactor, autovacuum_vacuum_threshold\n    );",
        &FormatOptions::default(),
    );
}

#[test]
fn hard_width_errors_use_document_line_numbers() {
    let source = "CREATE SCHEMA test;\n\nALTER TABLE public.long_table_name ALTER COLUMN long_column_name SET DEFAULT 123;";
    let options = FormatOptions {
        soft_line_width: 32,
        hard_line_width: 40,
        ..FormatOptions::default()
    };

    let formatted = format_sql(source, &options).expect("the failed statement is preserved");
    assert_eq!(formatted.output, source);
    let diagnostic = formatted
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.rule_id == "format.statement_skipped")
        .expect("the hard-width failure is diagnosed");
    assert!(diagnostic.message.contains("line 3"), "{diagnostic:?}");
}

#[test]
fn preserves_ddl_comments_at_element_and_action_boundaries() {
    let source = "CREATE TABLE users (\n    id bigint, -- stable identifier\n    email text,\n\n    -- table identity\n    CONSTRAINT users_pkey PRIMARY KEY (id)\n);\n\nALTER TABLE users\n    ADD COLUMN name text, -- display name\n    ADD COLUMN created_at timestamptz;\n";

    assert_format(source, source, &FormatOptions::default());
}

#[test]
fn neighboring_unowned_ddl_remains_unchanged() {
    for source in [
        "CREATE TABLE copied (LIKE source INCLUDING ALL);",
        "ALTER TABLE users RENAME COLUMN email TO primary_email;",
    ] {
        let formatted = format_sql_result(source, &FormatOptions::default());
        assert_eq!(formatted.output, source);
        assert!(!formatted.changed);
        assert_eq!(formatted.diagnostics.len(), 1);
        assert_eq!(formatted.diagnostics[0].rule_id, "syntax.unsupported");
    }
}
