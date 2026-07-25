SELECT
    jsonb_build_object(
        'identity', item.id, 'external_ids', jsonb_build_array(item.kp_id, item.imdb_id), 'title',
        COALESCE(item.title_rus, item.title_orig, '')
    ) AS payload,
    item.created_at
FROM public.items item;
