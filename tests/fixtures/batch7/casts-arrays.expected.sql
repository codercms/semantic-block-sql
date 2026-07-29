SELECT
    payload::public.custom_type[] AS typed_payload,
    CAST(created_at AS timestamp(3) WITH time zone) AS created_ts,
    CAST(payload AS public.other_type[]) AS other_payload,
    ARRAY[1, 2, 3]::integer[] AS ids,
    ARRAY[ARRAY[1, 2], ARRAY[3, 4]] AS matrix,
    tags[1] AS first_tag,
    tags[2:4] AS middle_tags,
    tags[:3] AS prefix_tags,
    tags[3:] AS suffix_tags,
    tags[:] AS all_tags,
    matrix[1][2] AS cell
FROM public.items
WHERE id = ANY (ARRAY[1, 2, 3]);

CREATE TABLE public.expression_fixture (
    id bigint,
    tags text[],
    matrix integer[][],
    payloads jsonb[]
);
