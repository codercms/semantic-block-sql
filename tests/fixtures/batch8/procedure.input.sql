CREATE PROCEDURE mark_item_done(item_id bigint)
LANGUAGE plpgsql
AS $procedure$
begin
update public.items set status='done' where id=item_id;
end;
$procedure$;
