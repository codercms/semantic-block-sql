DO $body$
declare
    item_id bigint:=1;
begin
    perform refresh_item(item_id);
end;
$body$;
