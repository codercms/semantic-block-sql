// semblock:sql
const RECENT: &str = r#"
    /* dashboard query */ select id,user_id,total from public.orders where created_at>=now()-interval '24 hours' order by created_at desc;
"#;

// semblock:ignore
const LEGACY: &str = r#"
    select id,total from legacy.orders where deleted_at is null;
"#;

const UPDATE_FROM_IMPORT: &str = r#"
    update public.orders item set total=source.total, updated_at=now() from staging.orders source where item.id=source.id and source.ready=true returning item.id, item.updated_at;
"#;

const DELETE_EXPIRED: &str = r#"
    delete from public.orders item using staging.orders source where item.id=source.id and source.expired=true returning item.id;
"#;

const GROUPED_QUERY: &str = r#"select id,total from public.orders;"#;
const MESSAGE: &str = r#"choose a plan from the menu"#;
const FRAGMENT: &str = r#"WHERE deleted_at IS NULL"#;
const INTERPRETED: &str = "select id,total from public.orders;";

pub fn queries() -> Vec<String> {
    let dynamic_columns = "id,total";
    let dynamic_query = format!("SELECT {} FROM public.orders", dynamic_columns);
    let mut queries = vec![RECENT, LEGACY, UPDATE_FROM_IMPORT, DELETE_EXPIRED,
        GROUPED_QUERY, MESSAGE, FRAGMENT, INTERPRETED].into_iter().map(str::to_owned).collect::<Vec<_>>();
    queries.push(dynamic_query);
    queries
}
