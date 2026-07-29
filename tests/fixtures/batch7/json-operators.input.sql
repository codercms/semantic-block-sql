select metadata->'profile' as profile, metadata->>'status' as status, metadata#>'{profile,tags}' as tags, metadata#>>'{profile,name}' as name, metadata#-'{obsolete,key}' as cleaned, metadata||'{"seen":true,"source":"manual"}'::jsonb as merged from public.items where metadata@>'{"status":"active"}'::jsonb and '{"status":"active"}'::jsonb<@metadata and metadata?'status' and metadata?|array['title','name'] and metadata?&array['id','status'];

update public.items set metadata=metadata||$1::jsonb where metadata->>'status'=$2 returning id, metadata#>>'{profile,name}' as name;

insert into public.events(payload) values($1::jsonb#-'{temporary}') returning payload->>'type' as event_type;
