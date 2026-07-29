DO $loops$
DECLARE
counter integer:=0;
item record;
item_id bigint;
item_ids bigint[];
item_cursor CURSOR FOR select id from items where active=true;
BEGIN
LOOP
EXIT WHEN done;
END LOOP;
WHILE counter < 10 LOOP
counter:=counter+1;
CONTINUE WHEN counter = 5;
END LOOP;
FOR item IN select id from items where active=true LOOP
PERFORM process(item.id);
END LOOP;
FOR index_value IN 1..10 BY 2 LOOP
PERFORM process(index_value);
END LOOP;
FOREACH item_id IN ARRAY item_ids LOOP
PERFORM process(item_id);
END LOOP;
<<scan_items>>
FOR item IN item_cursor LOOP -- loop stays lowercase in comment
CONTINUE scan_items WHEN item.id IS NULL;
EXIT scan_items WHEN item.id > 100;
END LOOP scan_items;
END;
$loops$;
