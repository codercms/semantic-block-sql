const FIND_BY_ID: &str = r#"
    select id,name,email from public.users where id=$1;
"#;
pub const INSERT_USER: &str = r#"insert into public.users (name,email) values ($1,$2) returning id;"#;
pub const DELETE_USER: &str = r#"delete from public.users where id=$1 returning id;"#;
pub static LIST_ACTIVE: &str = r#"
    select id,name,email from public.users where active=true order by name;
"#;

fn must_prepare(query: &str) -> &str { query }
fn consume(query: &str, _: i64) -> &str { query }

pub fn load(id: i64) -> &'static str {
    const LOOKUP: &str = r#"
        select id,name,email from public.users where id=$1;
	"#;
    let prepared = must_prepare(r#"
    select id,name from public.users where active=true;
"#);
    let mut query = r#"select id,name from public.users order by id;"#;
    std::hint::black_box(query);
    query = r#"select id,name from public.users where active=true order by id;"#;
    std::hint::black_box((query, prepared, LOOKUP));
    consume(r#"
        select id,name,email from public.users where active=true and id>=$1 order by id;
	"#, id)
}

pub fn return_active(minimum_id: i64) -> &'static str {
    consume(r#"
        select id,name from public.users where active=true and id>=$1;
	"#, minimum_id)
}

pub fn deactivate(id: i64) {
    consume(r#"
        update public.users set active=false where id=$1;
	"#, id);
}

pub fn find_by_id() -> &'static str { FIND_BY_ID }

pub const NORMALIZE_USER_TITLE: &str = r#"
create or replace procedure public.normalize_user_title(user_id bigint)
language plpgsql
as $function$
begin
update public.users set title=trim(title) where id=user_id;
end;
$function$;
"#;
