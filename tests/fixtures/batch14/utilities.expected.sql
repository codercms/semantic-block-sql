COPY items(id, name) FROM STDIN WITH (FORMAT csv);
CALL refresh_items(1, 'x');
EXPLAIN (ANALYZE, buffers, FORMAT json)
SELECT id, name FROM items WHERE active = TRUE;
VACUUM (FULL, ANALYZE, VERBOSE) items(id, name);
ANALYZE (VERBOSE, skip_locked) items(id, name);
REFRESH MATERIALIZED VIEW CONCURRENTLY mv_items WITH NO DATA;
LISTEN item_events;
NOTIFY item_events, 'payload';
CREATE EXTENSION IF NOT EXISTS pg_trgm WITH SCHEMA extensions VERSION '1.0' CASCADE;
ALTER TYPE mood ADD VALUE IF NOT EXISTS 'happy' AFTER 'ok';
ALTER DOMAIN positive_int ADD CONSTRAINT positive CHECK (VALUE > 0) NOT VALID;
ALTER POLICY tenant_policy ON items TO app_user
USING (tenant_id = current_setting('app.tenant')::uuid)
WITH CHECK (active = TRUE);
CREATE RULE notify_insert AS ON INSERT TO items
WHERE (new.active = TRUE)
DO ALSO NOTIFY item_events;
CREATE STATISTICS item_stats(dependencies, ndistinct) ON tenant_id, status FROM items;
CREATE COLLATION natural_sort(provider = icu, locale = 'und-u-kn-true', deterministic = FALSE);
CREATE COLLATION copied FROM existing_collation;
CREATE CAST(text AS uuid) WITH FUNCTION text_to_uuid(text) AS ASSIGNMENT;
CREATE SCHEMA IF NOT EXISTS app AUTHORIZATION app_user;
ALTER SEQUENCE item_id_seq RESTART WITH 100 INCREMENT BY 2;
ALTER INDEX idx_items RENAME TO idx_active_items;
ALTER MATERIALIZED VIEW mv_items RENAME TO mv_active_items;
ALTER TRIGGER audit_items ON items RENAME TO audit_active_items;
