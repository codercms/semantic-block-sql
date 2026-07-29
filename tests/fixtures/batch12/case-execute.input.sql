DO $$
DECLARE
query_text text:='select id from items where id=$1';
result bigint;
item_id bigint:=1;
active boolean:=true;
status text:='new';
BEGIN
CASE status
WHEN 'new' THEN
PERFORM queue_item();
ELSE
PERFORM archive_item();
END CASE;
CASE
WHEN active THEN
EXECUTE query_text INTO STRICT result USING item_id,active; -- using stays lowercase in comment
ELSE
result:=0;
END CASE;
END;
$$;
