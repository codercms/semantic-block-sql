CREATE OR REPLACE FUNCTION normalize_item(item_id bigint)
RETURNS bigint
LANGUAGE plpgsql
AS $$
declare
result_id bigint:=item_id;
begin
if result_id > 0 then
return result_id;
else
return 0;
end if;
exception
when unique_violation then
raise notice 'duplicate';
when others then
return -1;
end;
$$;
