# Manual formatting evaluation cases

Use these cases to test whether an agent applies the skill consistently. Compare structure and semantics rather than exact optional alignment.

## Case 1: mixed boolean logic

```sql
SELECT item.id,item.title_rus,item.title_orig FROM public.items item WHERE item.deleted_at IS NULL AND (item.title_rus IS NOT NULL OR item.title_orig IS NOT NULL) AND item.status IN ('released','announced') ORDER BY item.created_at DESC,item.id
```

Expected properties:

- uppercase keywords and special values;
- multiline `WHERE`;
- visible parenthesized `OR` group;
- compact `ORDER BY` if it fits;
- semicolon at the end.

## Case 2: `CASE`

```sql
SELECT item.id,CASE WHEN item.id IS NULL THEN 0 WHEN item.deleted_at IS NOT NULL THEN -1 ELSE first_expression+second_expression END AS score FROM public.items item
```

Expected properties:

- multiline `CASE`;
- compact branches;
- spaces around `+`;
- no semantic rewrite.

## Case 3: `ON CONFLICT`

```sql
INSERT INTO public.items (kp_id,title_rus,updated_at) VALUES ($1::bigint,$2::text,now()) ON CONFLICT (kp_id) WHERE kp_id IS NOT NULL DO UPDATE SET title_rus=EXCLUDED.title_rus,updated_at=now() WHERE items.title_rus IS DISTINCT FROM EXCLUDED.title_rus RETURNING id,kp_id,title_rus
```

Expected properties:

- `DO UPDATE` and `SET` structure is clear;
- conflict-target `WHERE` remains attached to `ON CONFLICT`;
- update-action `WHERE` remains later at statement scope;
- no casts are changed.

## Case 4: simple indexes

```sql
create index users_reg_date_idx on users(reg_date); create index active_users_reg_date_idx on users(reg_date) where deleted_at is null;
```

Expected properties:

- both statements may remain one line each;
- keywords uppercase;
- spaces before index column parentheses according to the style;
- no unnecessary multiline expansion.

## Case 5: `MERGE`

```sql
MERGE INTO target USING source ON target.id=source.id WHEN MATCHED AND source.deleted_at IS NOT NULL THEN DELETE WHEN MATCHED THEN UPDATE SET value=source.value WHEN NOT MATCHED THEN INSERT (id,value) VALUES (source.id,source.value)
```

Expected properties:

- separate `WHEN` branches with blank lines;
- action introducers stay on the `WHEN ... THEN` line;
- action arguments use one continuation indent, without a redundant `THEN` indentation level;
- assignments and values are spaced correctly;
- branch order is unchanged.

## Case 6: semantic safety

```sql
SELECT * FROM public.items WHERE status <> 'deleted' AND id=$1::bigint
```

Expected properties:

- `<>` may normalize to `!=`;
- no cast is added or removed;
- no predicate is reordered;
- no `SELECT *` rewrite.

## Case 7: expanded join condition without indentation storm

```sql
SELECT item.id FROM public.items item LEFT JOIN match_new.source_links link ON link.kp_id=item.kp_id AND link.status='approved' AND link.deleted_at IS NULL AND (link.model_version=$1::text OR link.match_method='manual')
```

Expected properties:

- `LEFT JOIN ... ON` stays on one owner line;
- predicates continue one indentation level below the join line;
- there is no line containing only `ON`;
- the parenthesized `OR` group is visible;
- semantics are unchanged.
