DO $$
DECLARE
item_id bigint;
bound_cursor CURSOR FOR select id from items where active=true;
dynamic_cursor refcursor;
BEGIN
OPEN bound_cursor;
FETCH NEXT FROM bound_cursor INTO item_id;
MOVE FORWARD 10 FROM bound_cursor;
CLOSE bound_cursor;
OPEN dynamic_cursor FOR select id from items where active=true order by id;
FETCH FIRST FROM dynamic_cursor INTO item_id;
CLOSE dynamic_cursor;
END;
$$;
