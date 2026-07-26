# Formatting evaluation cases

Evaluate semantic preservation and required layout. Optional alignment need not match exactly.

## 1. Mixed boolean logic

```sql
SELECT item.id,item.title_rus,item.title_orig FROM public.items item WHERE item.deleted_at IS NULL AND (item.title_rus IS NOT NULL OR item.title_orig IS NOT NULL) AND item.status IN ('released','announced')
```

Expected:

- uppercase keywords;
- spaces after commas;
- visible parenthesized `OR` group;
- no predicate reorder.

## 2. Authored groups and comments

```sql
SELECT
    item.id,item.kp_id,item.imdb_id,

    -- User-visible fields.
    item.title_rus,item.title_orig,

    item.created_at,item.updated_at
FROM public.items item
```

Expected:

- three groups preserved;
- comment remains attached to the following group;
- commas normalized;
- no column reorder.

## 3. `ON CONFLICT`

```sql
INSERT INTO items (kp_id,title) VALUES ($1::bigint,$2::text) ON CONFLICT (kp_id) WHERE kp_id IS NOT NULL DO UPDATE SET title=EXCLUDED.title WHERE items.deleted_at IS NULL
```

Expected:

- conflict-target `WHERE` remains with `ON CONFLICT`;
- `DO UPDATE`, `SET`, and action `WHERE` are distinct;
- casts unchanged.

## 4. `MERGE`

```sql
MERGE INTO target USING source ON target.id=source.id WHEN MATCHED THEN UPDATE SET value=source.value WHEN NOT MATCHED THEN INSERT (id,value) VALUES (source.id,source.value)
```

Expected:

- blank lines between `WHEN` branches;
- actions remain on `WHEN ... THEN` lines;
- branch order unchanged.

## 5. Casing whitelist

```sql
select count(*),sum(duration),coalesce(max(score),0),now(),extract(year from created_at),date_trunc('day',created_at) from items
```

Expected:

- `COUNT`, `SUM`, `COALESCE`, `MAX`, `NOW`, `EXTRACT` uppercase;
- `date_trunc` lowercase.

## 6. Semantic safety

```sql
SELECT * FROM items WHERE status <> 'deleted' AND id=$1::bigint
```

Expected:

- `<>` may remain unchanged;
- if the line is otherwise edited, `!=` is allowed but not required;
- cast, predicates, and `SELECT *` unchanged.

## 7. PL/pgSQL exception handlers

```sql
EXCEPTION WHEN foreign_key_violation THEN RAISE NOTICE 'missing'; WHEN unique_violation THEN RAISE NOTICE 'duplicate'; END;
```

Expected:

- `EXCEPTION` and `END` align;
- each `WHEN` is indented one level;
- handler statements are indented one more level;
- blank line between handlers.

## 8. Widths

Use generated list fixtures around 120 and 160 characters.

Expected:

- crossing 120 does not force an authored group to split;
- a safely breakable line beyond 160 is split;
- indivisible strings, identifiers, and comments are not split;
- formatting is idempotent.
