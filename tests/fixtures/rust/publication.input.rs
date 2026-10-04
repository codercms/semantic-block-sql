const VALIDATE_PUBLICATION_SQL: &str = r#"
SELECT
    NOT puballtables
    AND NOT EXISTS (
        SELECT 1
        FROM pg_publication_namespace pn
        JOIN pg_namespace n ON n.oid = pn.pnnspid
        WHERE pn.pnpubid = p.oid AND n.nspname IN ('cdc', 'etl')
    )
    AND NOT EXISTS (
        SELECT 1
        FROM pg_publication_tables pt
        WHERE pt.pubname = p.pubname AND pt.schemaname IN ('cdc', 'etl')
    )
FROM pg_publication p
WHERE p.pubname = $1
"#;
