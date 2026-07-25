SELECT
    item.id, item.kp_id, item.imdb_id,
    item.title_rus, item.title_orig,

    -- audit fields
    item.created_at, item.updated_at
FROM public.items item;
