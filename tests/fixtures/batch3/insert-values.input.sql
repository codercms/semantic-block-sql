insert into public.items (
id,title,created_at
)
values
($1,$2,now()),
($3,$4,now())
returning id,created_at;
