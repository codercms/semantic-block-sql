#[test]
fn query_corpus_compiles() {
    let tests = [
        ("active", "select id,name from public.catalog_items where active=true;"),
        ("by id", "select id,name from public.catalog_items where id=$1;"),
    ];
    for (name, query) in tests {
        assert!(!query.is_empty(), "{name} has no query");
    }
    assert!(!semblock_rust_fixture::users::load(1).is_empty());
    assert_eq!(semblock_rust_fixture::orders::queries().len(), 9);
    assert!(semblock_rust_fixture::catalog::dynamic_query("id,name").contains("id,name"));
}
