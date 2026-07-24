# Detailed PostgreSQL formatting reference

Read this reference for non-trivial SQL or when several valid layouts are possible.

## General model

Formatting follows the syntax tree. Every construct owns arguments, and every nested construct adds one four-space indentation level.

The formatter chooses between two shapes:

1. **Compact:** the owner and its arguments remain on one line.
2. **Expanded:** the owner introduces a nested block containing one argument or one logical argument group per line.

This model applies to clauses, functions, row constructors, CTEs, window definitions, DDL actions, `MERGE` branches, and procedural blocks.

## Line-breaking priority

When a line should be broken, prefer boundaries in this order:

1. top-level statement clauses;
2. top-level clause arguments;
3. logical boolean groups;
4. nested query boundaries;
5. function argument or JSON key/value group boundaries;
6. arithmetic or concatenation operators only when the expression is still unreadable.

Do not break inside identifiers, casts, qualified names, placeholders, string literals, or compact row comparisons.

## `SELECT`

A short list may remain inline:

```sql
SELECT id, kp_id, imdb_id
FROM public.items;
```

Expand when the list is long or contains substantial expressions:

```sql
SELECT
    item.id,
    item.kp_id,
    item.imdb_id,
    COALESCE(item.title_rus, item.title_orig, '') AS title,
    jsonb_build_object(
        'ratings', activity.rating_count,
        'average_rating', activity.average_rating,
        'notifications', activity.notification_count
    ) AS stats
FROM public.items item;
```

Logical groups may share lines or be separated by blank lines:

```sql
SELECT
    item.id, item.kp_id, item.imdb_id,
    COALESCE(item.title_rus, ''), COALESCE(item.title_orig, ''),
    item.created_at,

    activity.watch_count,
    activity.last_watched_at
FROM public.items item;
```

## Scalar subqueries and `EXISTS`

Use a nested block for a substantial scalar subquery:

```sql
SELECT
    item.id,
    (
        SELECT count(*)
        FROM public.user_watches watch
        WHERE watch.item_id = item.id
    ) AS watch_count
FROM public.items item;
```

Use the conventional `EXISTS (` shape:

```sql
WHERE EXISTS (
    SELECT 1
    FROM public.user_watches watch
    WHERE watch.item_id = item.id
)
```

A short inner query may remain compact when this improves the surrounding block.

## `WHERE`, `HAVING`, and `ON`

A short condition remains inline:

```sql
JOIN public.items item ON item.id = source.item_id AND item.deleted_at IS NULL
```

A long or structurally complex condition expands after `ON`. Keep `ON` attached to the join owner instead of creating an empty intermediate indentation level:

```sql
LEFT JOIN match_new.source_links link ON
    link.kp_id = item.kp_id
    AND link.status = 'approved'
    AND link.deleted_at IS NULL
    AND (
        link.model_version = current_model.version
        OR link.match_method = 'manual'
    )
```

Avoid this indentation-heavy shape:

```sql
LEFT JOIN match_new.source_links link
    ON
        link.kp_id = item.kp_id
        AND link.status = 'approved'
```

For expanded boolean expressions:

- continuation operators begin lines;
- operators at the same logical level have the same indentation;
- parenthesized child groups are indented one level;
- compact child groups may remain on one line.

## `GROUP BY`, `ORDER BY`, and windows

Keep short argument lists inline:

```sql
GROUP BY item.type, item.year
ORDER BY score DESC, item.id;
```

Expand when arguments are numerous or complex:

```sql
GROUP BY
    date_trunc('day', session.started_at),
    item.type,
    item.orig_src
ORDER BY
    date_trunc('day', session.started_at),
    item.type,
    item.orig_src;
```

A cohesive window specification may remain inline even when moderately long:

```sql
row_number() OVER (PARTITION BY item.type ORDER BY score DESC, item.id) AS position
```

Expand only when the window itself becomes difficult to scan.

## CTEs

Each CTE body is nested under its definition. Separate CTEs with commas after their closing parentheses:

```sql
WITH first_cte AS (
    SELECT ...
),
second_cte AS (
    SELECT ...
)
SELECT ...;
```

For recursive CTEs, use blank lines around the set operator to expose anchor and recursive terms.

## `INSERT` and `VALUES`

Keep a short target column list inline. Expand only when it becomes difficult to scan.

A single short row may stay inline:

```sql
VALUES ('manual', 'human', jsonb_build_object(), TRUE, now())
```

Multiple short rows:

```sql
VALUES
    ('manual', 'human', jsonb_build_object(), TRUE, now()),
    ('automatic', 'classifier', jsonb_build_object(), FALSE, now())
```

A complex row expands independently:

```sql
VALUES
    (
        'ml_v1',
        'classifier',
        jsonb_build_object(
            'threshold', 0.75,
            'features', jsonb_build_array('title', 'year')
        ),
        TRUE,
        now()
    ),
    ('manual', 'human', jsonb_build_object(), TRUE, now())
```

Rows do not need identical physical shapes when their complexity differs.

## `UPDATE`

For multiple assignments, place `SET` on its own line and indent assignments:

```sql
UPDATE public.items item
SET
    title_rus = source.title_rus,
    title_orig = source.title_orig,
    updated_at = now()
FROM staging.items source
WHERE source.id = item.id;
```

A short single assignment may remain inline when the complete statement is simple.

## `DELETE`

Keep a short `USING` list inline. Expand it according to the ordinary argument rules when necessary.

## `ON CONFLICT`

Canonical shape:

```sql
ON CONFLICT (version) DO UPDATE
SET
    config = EXCLUDED.config,
    is_active = EXCLUDED.is_active,
    updated_at = now()
```

Partial target:

```sql
ON CONFLICT (tenant_id, external_id)
    WHERE
        external_id IS NOT NULL
        AND source_type = 'imported'
DO UPDATE
SET
    ...
```

The target predicate is nested under `ON CONFLICT`. An action predicate appears later at statement scope:

```sql
DO UPDATE
SET
    ...
WHERE target.updated_at < EXCLUDED.updated_at
RETURNING target.id;
```

## `MERGE`

Treat each `WHEN` as a top-level branch. Keep the action introducer on the same line as `THEN`; only the action arguments receive one continuation indent:

```sql
MERGE INTO public.items target
USING staging.items source ON target.id = source.id

WHEN MATCHED AND source.deleted_at IS NOT NULL THEN DELETE

WHEN MATCHED THEN UPDATE SET
    title = source.title,
    updated_at = source.updated_at

WHEN NOT MATCHED THEN INSERT (id, title, updated_at)
    VALUES (source.id, source.title, source.updated_at);
```

For a complex merge condition, expand it after `ON` without adding an `ON`-only line:

```sql
USING staging.items source ON
    target.id = source.id
    AND target.tenant_id = source.tenant_id
    AND target.deleted_at IS NULL
```

This avoids indentation storms while preserving the branch and condition structure.

## Set operations

Use blank lines around set operators:

```sql
SELECT ...
FROM ...

UNION ALL

SELECT ...
FROM ...

EXCEPT

SELECT ...
FROM ...;
```

Never alter precedence. Preserve existing parentheses around compound set expressions.

## DDL

### `CREATE TABLE`

Put one column or table constraint per line. Use a blank line between columns and constraints:

```sql
CREATE TABLE stats.daily (
    item_id bigint NOT NULL,
    day date NOT NULL,
    watch_count bigint NOT NULL DEFAULT 0,

    CONSTRAINT daily_pk PRIMARY KEY (item_id, day),
    CONSTRAINT daily_watch_count_chk CHECK (watch_count >= 0)
);
```

Optional local alignment is acceptable but not required.

### `CREATE INDEX`

Keep a simple index on one line:

```sql
CREATE INDEX users_reg_date_idx ON users (reg_date);
CREATE INDEX users_reg_date_idx ON users (reg_date) WHERE (deleted_at IS NULL);
```

Use a multiline form when helpful:

```sql
CREATE INDEX item_watch_sessions_daily_active_idx
    ON stats.item_watch_sessions_daily (day DESC, item_id)
    INCLUDE (watch_count, watched_secs)
    WHERE
        watch_count > 0
        OR watched_secs > 0;
```

A compact multiline predicate is also valid when simple:

```sql
CREATE INDEX item_watch_sessions_daily_active_idx
    ON stats.item_watch_sessions_daily (day DESC, item_id)
    INCLUDE (watch_count, watched_secs)
    WHERE watch_count > 0 OR watched_secs > 0;
```

### `ALTER TABLE`

Indent actions beneath the statement. Blank lines may separate action categories:

```sql
ALTER TABLE match_new.source_links
    ADD COLUMN projection_status text NOT NULL DEFAULT 'pending',
    ADD COLUMN projection_error jsonb,

    ALTER COLUMN score SET DEFAULT 0,

    ADD CONSTRAINT source_links_projection_status_chk
        CHECK (
            projection_status IN ('pending', 'projected', 'blocked', 'failed')
        )
        NOT VALID,

    ADD CONSTRAINT source_links_kp_fk
        FOREIGN KEY (kp_id)
        REFERENCES kp_new.titles (id)
        ON UPDATE CASCADE
        ON DELETE RESTRICT;
```

## PL/pgSQL

Use four spaces for procedural nesting. Apply SQL formatting inside SQL statements:

```sql
DO $$
    DECLARE
        affected_rows bigint;
        current_version text;
    BEGIN
        SELECT version INTO current_version
        FROM match_new.model_versions
        WHERE is_active
        ORDER BY created_at DESC
        LIMIT 1;

        IF current_version IS NULL THEN
            RAISE EXCEPTION 'active model version is missing';
        END IF;

    EXCEPTION
        WHEN unique_violation THEN
            RAISE NOTICE 'audit event already exists';
    END
$$;
```

Do not parse or modify unknown languages embedded in dollar-quoted strings.

## Comments

Preserve comment text. Keep comments attached to the same logical statement or expression.

Use a comment on its own line when it explains the following block:

```sql
-- Keep manually approved links even when the model score is low.
WHERE
    decision.decision = 'approved'
    OR link.score >= $1::float8
```

Do not move a trailing comment to another expression if doing so could change its apparent subject.
