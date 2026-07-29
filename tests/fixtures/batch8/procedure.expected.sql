CREATE PROCEDURE mark_item_done(item_id bigint)
LANGUAGE plpgsql
AS $procedure$
BEGIN
    UPDATE public.items SET status = 'done' WHERE id = item_id;
END;
$procedure$;
