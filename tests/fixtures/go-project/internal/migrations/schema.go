package migrations

const Schema = `
    create type public.event_status as enum ('pending','ready','failed');
    create table public.events (id bigint,status public.event_status,created_at timestamptz) partition by range(created_at) using heap with(fillfactor=80);
    create table public.events_2026 partition of public.events for values from ('2026-01-01') to ('2027-01-01');
    grant select,insert,update on table public.events to app_user;
`

const ClaimQuery = `
    with claimed as (update public.events set status='ready' where id in (select id from public.events where status='pending' for update skip locked) returning id,status) select id,status from claimed order by id;
`

const Maintenance = `
do $maintenance$
declare
query_text text:='update public.events set status=$1 where id=$2';
event_id bigint;
begin
for event_id in select id from public.events where status='pending' loop
execute query_text using 'ready',event_id;
end loop;
end;
$maintenance$;
`
