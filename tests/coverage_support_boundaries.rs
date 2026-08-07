mod support;

use support::{SqlCase, assert_cases, assert_unsupported_cases};

#[test]
fn formats_the_reviewed_neighbor_of_each_high_risk_boundary() {
    assert_cases(&[
        SqlCase::new(
            "materialized-view storage options",
            "create materialized view item_counts using heap with(fillfactor=90) tablespace fast as select count(*) as total from items with no data;",
            "CREATE MATERIALIZED VIEW item_counts\nUSING heap\nWITH (fillfactor = 90)\nTABLESPACE fast AS\nSELECT COUNT(*) AS total\nFROM items\nWITH NO DATA;",
        ),
        SqlCase::new(
            "partitioned table",
            "create table events(id bigint,tenant_id bigint) partition by hash(tenant_id);",
            "CREATE TABLE events (\n    id bigint,\n    tenant_id bigint\n)\nPARTITION BY HASH (tenant_id);",
        ),
        SqlCase::new(
            "partition child",
            "create table events_0 partition of events for values with(modulus 4,remainder 0);",
            "CREATE TABLE events_0 PARTITION OF events\nFOR VALUES WITH (modulus 4, remainder 0);",
        ),
        SqlCase::new(
            "single-statement SQL routine",
            "CREATE FUNCTION one() RETURNS int LANGUAGE SQL BEGIN ATOMIC select 1; END;",
            "CREATE FUNCTION one() RETURNS int LANGUAGE SQL BEGIN ATOMIC\n    SELECT 1;\nEND;",
        ),
    ]);
}

#[test]
fn preserves_adjacent_valid_but_unreviewed_syntax_byte_identically() {
    assert_unsupported_cases(&[
        (
            "view query beginning with WITH",
            "CREATE VIEW v AS WITH source AS (SELECT 1 AS id) SELECT id FROM source;",
        ),
        (
            "materialized-view query beginning with WITH",
            "CREATE MATERIALIZED VIEW mv AS WITH source AS (SELECT 1 AS id) SELECT id FROM source WITH DATA;",
        ),
        (
            "subpartition declaration",
            "CREATE TABLE child PARTITION OF parent FOR VALUES FROM (1) TO (10) PARTITION BY HASH (tenant_id);",
        ),
        (
            "CREATE TABLE LIKE",
            "CREATE TABLE copied (LIKE source INCLUDING ALL);",
        ),
        (
            "CREATE TABLE AS",
            "CREATE TABLE snapshot AS SELECT * FROM source;",
        ),
        (
            "publication DDL",
            "CREATE PUBLICATION pub FOR TABLE items WITH (publish = 'insert, update');",
        ),
        (
            "subscription DDL",
            "CREATE SUBSCRIPTION sub CONNECTION 'host=localhost' PUBLICATION pub;",
        ),
        (
            "XMLTABLE relation source",
            "SELECT * FROM XMLTABLE('/rows/row' PASSING doc COLUMNS id int PATH '@id') x;",
        ),
        (
            "JSON_TABLE expression",
            "SELECT * FROM JSON_TABLE(doc, '$[*]' COLUMNS (id int PATH '$.id')) jt;",
        ),
        (
            "multi-statement SQL-standard routine",
            "CREATE FUNCTION f() RETURNS void LANGUAGE SQL BEGIN ATOMIC SELECT 1; SELECT 2; END;",
        ),
    ]);
}
