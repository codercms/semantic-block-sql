mod support;

use support::{SqlCase, assert_cases};

#[test]
fn formats_jsonpath_and_hstore_operators_across_expression_owners() {
    assert_cases(&[
        SqlCase::new(
            "JSONPath SELECT target",
            "select doc @? '$.a' as is_match from events;",
            "SELECT doc @? '$.a' AS is_match FROM events;",
        ),
        SqlCase::new(
            "JSONPath predicate",
            "select id from events where doc @@ '$.enabled == true';",
            "SELECT id FROM events WHERE doc @@ '$.enabled == true';",
        ),
        SqlCase::new(
            "JSONPath JOIN predicate",
            "select * from events e join rules r on e.doc @? r.path and e.doc @@ r.predicate;",
            "SELECT *\nFROM events e\nJOIN rules r ON e.doc @? r.path AND e.doc @@ r.predicate;",
        ),
        SqlCase::new(
            "JSONPath UPDATE and RETURNING",
            "update events set is_match=doc @? '$.a' where doc @@ '$.enabled == true' returning doc @? '$.a';",
            "UPDATE events SET is_match = doc @? '$.a' WHERE doc @@ '$.enabled == true' RETURNING doc @? '$.a';",
        ),
        SqlCase::new(
            "JSONPath CHECK",
            "create table events (doc jsonb,constraint doc_chk check(doc @? '$.id'));",
            "CREATE TABLE events (\n    doc jsonb,\n\n    CONSTRAINT doc_chk CHECK (doc @? '$.id')\n);",
        ),
        SqlCase::new(
            "hstore UPDATE",
            "update items set attrs=attrs||patch,removed=attrs-'obsolete' where attrs@>'active=>true'::hstore returning attrs->'key';",
            "UPDATE items\nSET attrs = attrs || patch, removed = attrs - 'obsolete'\nWHERE attrs @> 'active=>true'::hstore\nRETURNING attrs -> 'key';",
        ),
        SqlCase::new(
            "hstore CHECK",
            "create table items (attrs hstore check(attrs?'id' and attrs@>'active=>true'::hstore));",
            "CREATE TABLE items (\n    attrs hstore CHECK (attrs ? 'id' AND attrs @> 'active=>true'::hstore)\n);",
        ),
    ]);
}

#[test]
fn formats_high_variation_postgresql_operator_families() {
    assert_cases(&[
        SqlCase::new(
            "hstore operator family",
            "select attrs->'key',attrs?'key',attrs?|array['a','b'],attrs?&array['a','b'],attrs@>'a=>1'::hstore,attrs<@other,attrs||patch,attrs-'obsolete' from items;",
            "SELECT\n    attrs -> 'key',\n    attrs ? 'key',\n    attrs ?| ARRAY['a', 'b'],\n    attrs ?& ARRAY['a', 'b'],\n    attrs @> 'a=>1'::hstore,\n    attrs <@ other,\n    attrs || patch,\n    attrs - 'obsolete'\nFROM items;",
        ),
        SqlCase::new(
            "array operators",
            "select tags&&array['a'],tags@>array['a'],tags<@array['a','b'] from items;",
            "SELECT tags && ARRAY['a'], tags @> ARRAY['a'], tags <@ ARRAY['a', 'b'] FROM items;",
        ),
        SqlCase::new(
            "range operators",
            "select span&&int4range(1,10),span<<int4range(10,20),span>>int4range(-10,0),span&<int4range(5,20),span&>int4range(-5,5),span-|-int4range(10,20) from ranges;",
            "SELECT\n    span && int4range(1, 10),\n    span << int4range(10, 20),\n    span >> int4range(-10, 0),\n    span &< int4range(5, 20),\n    span &> int4range(-5, 5),\n    span -|- int4range(10, 20)\nFROM ranges;",
        ),
        SqlCase::new(
            "network operators",
            "select network<<inet '10.0.0.0/8',network<<=inet '10.0.0.0/8',network>>inet '10.0.0.1',network>>=inet '10.0.0.0/8',network&&inet '10.0.0.0/24' from hosts;",
            "SELECT\n    network << inet '10.0.0.0/8',\n    network <<= inet '10.0.0.0/8',\n    network >> inet '10.0.0.1',\n    network >>= inet '10.0.0.0/8',\n    network && inet '10.0.0.0/24'\nFROM hosts;",
        ),
        SqlCase::new(
            "full-text search operators",
            "select document@@query,query&&other_query,query||other_query,!!query,query<->other_query from search_documents;",
            "SELECT document @@ query, query && other_query, query || other_query, !! query, query <-> other_query\nFROM search_documents;",
        ),
        SqlCase::new(
            "regular-expression operators",
            "select name~'^a',name~*'^a',name!~'^a',name!~*'^a' from users;",
            "SELECT name ~ '^a', name ~* '^a', name !~ '^a', name !~* '^a' FROM users;",
        ),
    ]);
}

#[test]
fn uppercases_contextual_operator_grammar_without_reclassifying_types() {
    assert_cases(&[
        SqlCase::new(
            "pattern and truth predicates",
            "select name like 'a%',name ilike 'a%',name similar to '(a|b)%',value is unknown from items;",
            "SELECT name LIKE 'a%', name ILIKE 'a%', name SIMILAR TO '(a|b)%', value IS UNKNOWN FROM items;",
        ),
        SqlCase::new(
            "AT TIME ZONE",
            "select created_at at time zone 'UTC',cast(created_at as timestamp with time zone) from items;",
            "SELECT created_at AT TIME ZONE 'UTC', CAST(created_at AS timestamp WITH time zone) FROM items;",
        ),
        SqlCase::new(
            "schema-qualified custom operator",
            "select left_value operator(public.===) right_value from pairs;",
            "SELECT left_value OPERATOR (public.===) right_value FROM pairs;",
        ),
        SqlCase::new(
            "named arguments",
            "select calculate(value=>1,mode=>'fast');",
            "SELECT calculate(value => 1, mode => 'fast');",
        ),
        SqlCase::new(
            "collation syntax",
            "select title collate \"C\" from items order by title collate \"C\";",
            "SELECT title COLLATE \"C\" FROM items ORDER BY title COLLATE \"C\";",
        ),
        SqlCase::new(
            "CREATE LANGUAGE",
            "create language plpython3u;",
            "CREATE LANGUAGE plpython3u;",
        ),
        SqlCase::new(
            "contextual words remain identifiers",
            "select language,operator,unknown,zone from settings where language='sql';",
            "SELECT language, operator, unknown, zone FROM settings WHERE language = 'sql';",
        ),
    ]);
}
