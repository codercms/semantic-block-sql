package orders

// semblock:sql
const recent = `
    /* dashboard query */ select id,user_id,total from public.orders where created_at>=now()-interval '24 hours' order by created_at desc;
`

// semblock:ignore
const legacy = `
    select id,total from legacy.orders where deleted_at is null;
`

const updateFromImport = `
    update public.orders item set total=source.total, updated_at=now() from staging.orders source where item.id=source.id and source.ready=true returning item.id, item.updated_at;
`

const deleteExpired = `
    delete from public.orders item using staging.orders source where item.id=source.id and source.expired=true returning item.id;
`

const message = `choose a plan from the menu`
const fragment = `WHERE deleted_at IS NULL`
const interpreted = "select id,total from public.orders;"

func Queries() []string {
	return []string{recent, legacy, updateFromImport, deleteExpired, message, fragment, interpreted}
}
