select COUNT(item.id), item.value::BIGINT from public.items item where item.id <> 1;
