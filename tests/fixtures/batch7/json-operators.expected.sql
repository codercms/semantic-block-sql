SELECT
    metadata -> 'profile' AS profile,
    metadata ->> 'status' AS status,
    metadata #> '{profile,tags}' AS tags,
    metadata #>> '{profile,name}' AS name,
    metadata #- '{obsolete,key}' AS cleaned,
    metadata || '{"seen":true,"source":"manual"}'::jsonb AS merged
FROM public.items
WHERE
    metadata @> '{"status":"active"}'::jsonb
    AND '{"status":"active"}'::jsonb <@ metadata
    AND metadata ? 'status'
    AND metadata ?| ARRAY['title', 'name']
    AND metadata ?& ARRAY['id', 'status'];

UPDATE public.items
SET metadata = metadata || $1::jsonb
WHERE metadata ->> 'status' = $2
RETURNING id, metadata #>> '{profile,name}' AS name;

INSERT INTO public.events (payload) VALUES ($1::jsonb #- '{temporary}') RETURNING payload ->> 'type' AS event_type;
