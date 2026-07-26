# Compact examples

Use these only when the core rules leave the intended layout unclear.

## Long list without authored groups

Input:

```sql
SELECT item.id,item.kp_id,item.imdb_id,item.title_rus,item.title_orig,item.created_at FROM public.items item
```

Output:

```sql
SELECT
    item.id,
    item.kp_id,
    item.imdb_id,
    item.title_rus,
    item.title_orig,
    item.created_at
FROM public.items item;
```

## Preserved authored groups

```sql
SELECT
    item.id, item.kp_id, item.imdb_id,

    -- User-visible fields.
    item.title_rus, item.title_orig,

    item.created_at, item.updated_at
FROM public.items item;
```

Do not merge or invent groups.

## Complex boolean logic

```sql
WHERE
    item.deleted_at IS NULL
    AND item.status = 'active'
    AND (
        item.title_rus IS NOT NULL
        OR item.title_orig IS NOT NULL
    )
```

## Compact and expanded `CASE`

```sql
status = CASE WHEN approved THEN 'approved' ELSE 'rejected' END
```

```sql
CASE
    WHEN item.id IS NULL THEN 0
    WHEN item.deleted_at IS NOT NULL THEN -1
    ELSE score
END
```

## Casing

```sql
SELECT
    COUNT(*) AS item_count,
    COALESCE(MAX(score), 0) AS max_score,
    EXTRACT(YEAR FROM created_at) AS created_year,
    NOW() + INTERVAL '5 minutes' AS expires_at,
    date_trunc('day', created_at) AS created_day
FROM public.items;
```

`date_trunc` remains lowercase.

## Comments

```sql
-- Keep manually approved links.
WHERE decision = 'approved'
```

```sql
AND status = 'active' -- Legacy compatibility.
```

Do not move either comment to another expression.
