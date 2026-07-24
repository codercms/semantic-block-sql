select source.id from staging.source source join public.items item on item.id = source.item_id;
