package orders

const (
	ListQuery = `
        select order_item.id,order_item.customer_id,order_item.total
        from public.orders order_item
        where order_item.cancelled_at is null
        order by order_item.id;
    `

	UpsertQuery = `
        insert into public.orders (id,customer_id,total) values ($1,$2,$3) on conflict (id) do update set total=excluded.total returning id;
    `
)

const DeleteQuery = `delete from public.orders where id=$1 returning id;`

const CommentedUpdateQuery = `
    update public.orders set total=$1, -- chosen total
    updated_at=now() where id=$2;
`

var PreparedQuery = mustPrepare(`
    select id,customer_id,total from public.orders where id=$1;
`)

const WindowsPathQuery = `select 'C:\tmp\orders' as path,'line\nbreak' as escaped;`
const QuotedIdentifierQuery = `select "select","CamelCase" from "Order";`
const WhereFragment = `WHERE cancelled_at IS NULL`
const InterpretedQuery = "select id from public.orders;"

// semblock:ignore
const DynamicQuery = `select ` + "id" + ` from public.orders`

func mustPrepare(query string) string {
	return query
}

func ReassignedQuery() string {
	query := `
        select id,total from public.orders where customer_id=$1;
    `
	query = `
        select id,total from public.orders where customer_id=$1 and cancelled_at is null;
    `
	return query
}
