package audit

func consume(query string, arguments ...any) {}

func Record(id int64) {
	defer consume("insert into public.audit_log(entity_id,event_name) values($1,$2);", id, "closed")
	go consume("select id,event_name from public.audit_log where entity_id=$1 order by id;", id)
}
