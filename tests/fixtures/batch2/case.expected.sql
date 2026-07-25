SELECT
    CASE
        WHEN item.id IS NULL THEN 0
        WHEN item.deleted_at IS NOT NULL THEN -1
        ELSE item.score + 1
    END AS priority,
    status = CASE WHEN source.approved THEN 'approved' ELSE 'rejected' END
FROM public.items item
JOIN staging.source source ON source.id = item.id;
