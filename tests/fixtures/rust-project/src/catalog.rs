pub struct Queries {
    pub find_active: &'static str,
    pub archive: &'static str,
}

fn wrap(query: &str) -> &str { query }
fn consume(query: &str, _: i64) -> &str { query }

pub fn find_active(minimum_id: i64) -> &'static str {
    consume("select id,name,metadata from public.catalog_items where active=true and id>=$1 order by id;", minimum_id)
}

pub fn archive(id: i64) -> &'static str {
    consume(wrap("update public.catalog_items set archived_at=now() where id=$1 returning id;"), id)
}

pub fn default_queries() -> Queries {
    Queries {
        find_active: "select id,name from public.catalog_items where active=true order by name;",
        archive: "update public.catalog_items set archived_at=now() where id=$1;",
    }
}

pub fn static_query() -> &'static str {
    "SELECT id,name FROM public.catalog_items WHERE active=true AND id>0;"
}

pub fn dynamic_query(columns: &str) -> String {
    format!("SELECT {} FROM public.catalog_items", columns)
}

const MARKER_QUERY: &str = "select '`' as marker,id from public.catalog_items where id=$1;";
pub fn marker_query() -> &'static str { MARKER_QUERY }
