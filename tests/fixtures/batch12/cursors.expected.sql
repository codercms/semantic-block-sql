DO $$
DECLARE
    item_id bigint;
    bound_cursor CURSOR FOR SELECT id FROM items WHERE active = TRUE;
    dynamic_cursor refcursor;
BEGIN
    OPEN bound_cursor;
    FETCH NEXT FROM bound_cursor INTO item_id;
    MOVE FORWARD 10 FROM bound_cursor;
    CLOSE bound_cursor;
    OPEN dynamic_cursor FOR SELECT id FROM items WHERE active = TRUE ORDER BY id;
    FETCH FIRST FROM dynamic_cursor INTO item_id;
    CLOSE dynamic_cursor;
END;
$$;
