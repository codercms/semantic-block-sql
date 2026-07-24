SELECT count(item.id), item.value::bigint FROM public.items item WHERE item.id != 1;
