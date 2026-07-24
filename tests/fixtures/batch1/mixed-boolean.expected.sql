SELECT item.id
FROM public.items item
WHERE
    item.deleted_at IS NULL
    AND (
        item.title_rus IS NOT NULL
        OR item.title_orig IS NOT NULL
    );
