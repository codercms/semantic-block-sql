DO $body$
DECLARE
    item_id bigint := 1;
BEGIN
    PERFORM refresh_item(item_id);
END;
$body$;
