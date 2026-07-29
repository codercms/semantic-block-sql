select payload :: Public.Custom_Type [] as typed_payload, cast(created_at as TIMESTAMP(3) with time zone) as created_ts, cast(payload as Public.Other_Type []) as other_payload, array[1,2,3] :: INTEGER [] as ids, array[array[1,2],array[3,4]] as matrix, tags[1] as first_tag, tags[2:4] as middle_tags, tags[:3] as prefix_tags, tags[3:] as suffix_tags, tags[:] as all_tags, matrix[1][2] as cell from public.items where id = any(array[1,2,3]);

create table public.expression_fixture (id bigint, tags TEXT [], matrix INTEGER [][], payloads jsonb []);
