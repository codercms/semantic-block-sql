SELECT item.id
FROM public.items item
LEFT JOIN match_new.source_links link ON
    link.kp_id = item.kp_id
    AND link.status = 'approved'
    AND (
        link.model_version = item.model_version
        OR link.match_method = 'manual'
    );
