SELECT source.id
FROM staging.source source
JOIN public.items item ON item.id = source.item_id;
