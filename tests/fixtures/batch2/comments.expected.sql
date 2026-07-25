-- selected identity
SELECT
    item.id, -- stable id
    /* external ids */
    item.kp_id, item.imdb_id
FROM public.items item;
