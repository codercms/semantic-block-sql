# Statement layout reference

Read this file only for non-trivial or ambiguous statement shapes. Mandatory casing, spacing, width, grouping, and safety rules are in `SKILL.md`.

## `SELECT` and subqueries

```sql
SELECT id, kp_id, imdb_id
FROM public.items;
```

```sql
SELECT
    item.id,
    (
        SELECT COUNT(*)
        FROM public.user_watches watch
        WHERE watch.item_id = item.id
    ) AS watch_count
FROM public.items item;
```

Preserve existing alias syntax. Never rename, add, or remove aliases only for formatting.

## `WHERE`, `HAVING`, and `JOIN ... ON`

```sql
WHERE batch_id = $1::uuid AND validation_error IS NULL
```

```sql
LEFT JOIN source_links link ON
    link.item_id = item.id
    AND (
        link.status = 'approved'
        OR link.match_method = 'manual'
    )
```

## CTEs

```sql
WITH totals AS (
    SELECT item_id, COUNT(*) AS sessions
    FROM stats.sessions
    GROUP BY item_id
),
ranked AS (
    SELECT
        item_id,
        row_number() OVER (ORDER BY sessions DESC) AS position
    FROM totals
)
SELECT item_id, position
FROM ranked;
```

Use blank lines around the set operator between recursive anchor and recursive branches.

## Set operations

```sql
(
    SELECT id
    FROM active_items

    UNION ALL

    SELECT id
    FROM archived_items
)

EXCEPT

SELECT id
FROM blocked_items;
```

```sql
SELECT id
FROM active_items

UNION ALL

(
    SELECT id
    FROM archived_items
    ORDER BY archived_at DESC
    LIMIT 100
);
```

## `INSERT` and `VALUES`

```sql
VALUES
    (
        'ml_v1',
        jsonb_build_object(
            'threshold', 0.75,
            'strict', TRUE
        ),
        NOW()
    ),
    ('manual', jsonb_build_object(), NOW());
```

## `ON CONFLICT`

```sql
ON CONFLICT (kp_id) WHERE kp_id IS NOT NULL
DO UPDATE
SET
    title = EXCLUDED.title,
    updated_at = NOW()
WHERE items.deleted_at IS NULL
```

```sql
ON CONFLICT (tenant_id, external_id) WHERE
    external_id IS NOT NULL
    AND source_type = 'imported'
DO UPDATE
SET
    payload = EXCLUDED.payload;
```

## `UPDATE` and `DELETE`

```sql
UPDATE public.items item
SET
    title = source.title,
    updated_at = NOW()
FROM staging.items source
WHERE source.id = item.id;
```

```sql
DELETE FROM public.items item
USING staging.deleted_items source
WHERE
    source.id = item.id
    AND item.deleted_at IS NOT NULL
RETURNING item.id;
```

## `MERGE`

```sql
MERGE INTO public.items target
USING staging.items source ON target.id = source.id

WHEN MATCHED AND source.deleted_at IS NOT NULL THEN DELETE

WHEN MATCHED THEN UPDATE SET
    title = source.title,
    updated_at = NOW()

WHEN NOT MATCHED THEN INSERT (id, title)
    VALUES (source.id, source.title);
```

Complex match condition:

```sql
USING staging.items source ON
    target.id = source.id
    AND target.tenant_id = source.tenant_id
```

## DDL

```sql
CREATE INDEX users_reg_date_idx ON users (reg_date);
CREATE INDEX active_users_idx ON users (reg_date) WHERE deleted_at IS NULL;
```

```sql
CREATE INDEX item_activity_idx
    ON stats.item_activity (created_at DESC, item_id)
    INCLUDE (watch_count, rating_count)
    WHERE
        watch_count > 0
        OR rating_count > 0;
```

```sql
CREATE TABLE stats.daily (
    item_id bigint NOT NULL,
    day date NOT NULL,
    watch_count bigint NOT NULL DEFAULT 0,

    CONSTRAINT daily_pk PRIMARY KEY (item_id, day),
    CONSTRAINT daily_count_chk CHECK (watch_count >= 0)
);
```

```sql
ALTER TABLE public.items
    ADD COLUMN projection_status text NOT NULL DEFAULT 'pending',
    ADD COLUMN projection_error jsonb,

    ALTER COLUMN score SET DEFAULT 0,

    ADD CONSTRAINT projection_status_chk
        CHECK (projection_status IN ('pending', 'done', 'failed'));
```

## Functions and procedures

```sql
CREATE OR REPLACE FUNCTION active_item_count()
RETURNS bigint
LANGUAGE SQL STABLE
BEGIN ATOMIC
    SELECT COUNT(*)
    FROM public.items
    WHERE deleted_at IS NULL;
END;
```

```sql
CREATE OR REPLACE FUNCTION normalize_title(title text)
RETURNS text
LANGUAGE plpgsql IMMUTABLE
AS $$
BEGIN
    RETURN NULLIF(trim(title), '');
END;
$$;
```

```sql
IF item.deleted_at IS NOT NULL THEN
    RETURN;
ELSIF item.status = 'pending' THEN
    PERFORM refresh_item(item.id);
ELSE
    RAISE NOTICE 'Already processed';
END IF;
```

```sql
EXCEPTION
    WHEN foreign_key_violation THEN
        RAISE NOTICE 'Item does not exist';

    WHEN unique_violation THEN
        RAISE NOTICE 'Item already exists';
END;
```

## Embedded and templated SQL

```go
func loadItems() {
    const query = `
SELECT id, title
FROM public.items
WHERE deleted_at IS NULL
`
}
```

```sql
SELECT id, updated_at
FROM {{ ref('items') }}
{% if is_incremental() %}
WHERE updated_at > (
    SELECT MAX(updated_at)
    FROM {{ this }}
)
{% endif %}
```
