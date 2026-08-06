CREATE OR REPLACE FUNCTION active_items(limit_count bigint = NULL)
RETURNS TABLE (item_id bigint)
LANGUAGE SQL STABLE STRICT
BEGIN ATOMIC
    -- Include current and archived rows.
    WITH active AS (
        (SELECT id FROM public.items WHERE deleted_at IS NULL)

        UNION ALL (

        SELECT id FROM archived_items WHERE deleted_at IS NULL)
    )
    SELECT id
    FROM active
    WHERE limit_count IS NULL OR id <= limit_count;
END;
