#![allow(dead_code)]

struct Row {
    id: i64,
    name: Option<String>,
}

// All SQL is reused verbatim from the existing Go fixtures/tests.
pub fn queries() {
    let _ = sqlx::query!("select id,name from public.catalog_items where id=$1;", 1_i64);
    let _ = sqlx::query_unchecked!("select id,name from public.catalog_items where id=$1;", 1_i64);
    let _ = sqlx::query_as!(Row, r#"select id,name from public.catalog_items where id=$1;"#, 1_i64);
    let _ = sqlx::query_as_unchecked!(Row, r#"select id,name from public.catalog_items where id=$1;"#, 1_i64);
    let _ = sqlx::query_scalar!("select id from audit_log;");
    let _ = sqlx::query_scalar_unchecked!("select id from audit_log;");
    let _ = sqlx::query::<sqlx::Postgres>("select id,name from public.catalog_items where id=$1;").bind(1_i64);
}
