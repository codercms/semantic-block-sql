fn consume(_: &str, _: i64) {}

pub fn record(id: i64) {
    consume("insert into public.audit_log(entity_id,event_name) values($1,$2);", id);
    std::thread::spawn(move || {
        consume("select id,event_name from public.audit_log where entity_id=$1 order by id;", id);
    });
}
