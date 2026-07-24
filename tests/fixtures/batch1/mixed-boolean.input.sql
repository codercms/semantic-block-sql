select item.id from public.items item where item.deleted_at is null and (item.title_rus is not null or item.title_orig is not null);
