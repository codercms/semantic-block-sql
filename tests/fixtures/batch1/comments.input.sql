-- query purpose
select item.id, /* stable attachment */ item.title from public.items item where item.title = $$literal SELECT and <>$$;
