DO $body$
declare
    item_id bigint:=1;
begin
    -- Keep this attached to the following statement.
    perform refresh_item(item_id);
end;
$body$;
