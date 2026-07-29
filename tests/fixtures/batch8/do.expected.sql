DO $body$
DECLARE
    item_id bigint := 1;
BEGIN
    -- Keep this attached to the following statement.
    PERFORM refresh_item(item_id);
END;
$body$;
