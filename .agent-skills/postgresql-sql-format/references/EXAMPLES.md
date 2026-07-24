# Formatting examples

Use these examples to resolve layout ambiguity. They are illustrative rather than exact templates for every query.

## CTEs and window functions

### Input

```sql
WITH session_totals AS (SELECT s.item_id,count(*) AS sessions,sum(s.watched_secs) AS watched_secs,max(s.created_at) AS last_watched_at FROM stats.user_watch_sessions s WHERE s.created_at >= $1::timestamptz AND s.created_at < $2::timestamptz GROUP BY s.item_id), ranked_items AS (SELECT st.item_id,st.sessions,st.watched_secs,st.last_watched_at,row_number() OVER (PARTITION BY item.type ORDER BY st.watched_secs DESC,st.sessions DESC,item.id) AS position FROM session_totals st JOIN public.items item ON item.id=st.item_id AND item.deleted_at IS NULL) SELECT r.item_id,item.title_rus,item.title_orig,r.sessions,r.position FROM ranked_items r JOIN public.items item ON item.id=r.item_id WHERE r.position <= $3::integer AND (item.title_rus IS NOT NULL OR item.title_orig IS NOT NULL) ORDER BY item.type,r.position,item.id
```

### Output

```sql
WITH session_totals AS (
    SELECT
        s.item_id,
        count(*) AS sessions,
        sum(s.watched_secs) AS watched_secs,
        max(s.created_at) AS last_watched_at
    FROM stats.user_watch_sessions s
    WHERE s.created_at >= $1::timestamptz AND s.created_at < $2::timestamptz
    GROUP BY s.item_id
),
ranked_items AS (
    SELECT
        st.item_id,
        st.sessions,
        st.watched_secs,
        st.last_watched_at,
        row_number() OVER (PARTITION BY item.type ORDER BY st.watched_secs DESC, st.sessions DESC, item.id) AS position
    FROM session_totals st
    JOIN public.items item ON item.id = st.item_id AND item.deleted_at IS NULL
)
SELECT r.item_id, item.title_rus, item.title_orig, r.sessions, r.position
FROM ranked_items r
JOIN public.items item ON item.id = r.item_id
WHERE
    r.position <= $3::integer
    AND (item.title_rus IS NOT NULL OR item.title_orig IS NOT NULL)
ORDER BY item.type, r.position, item.id;
```

## Recursive CTE

```sql
WITH RECURSIVE category_tree AS (
    SELECT c.id, c.parent_id, c.name, 0 AS depth, ARRAY[c.id] AS path
    FROM catalog.categories c
    WHERE c.parent_id IS NULL AND c.deleted_at IS NULL

    UNION ALL

    SELECT child.id, child.parent_id, child.name, parent.depth + 1, parent.path || child.id
    FROM catalog.categories child
    JOIN category_tree parent ON parent.id = child.parent_id
    WHERE child.deleted_at IS NULL AND NOT child.id = ANY (parent.path)
)
SELECT id, parent_id, name, depth, path
FROM category_tree
WHERE depth <= $1::integer
ORDER BY path;
```

## `INSERT ... ON CONFLICT`

```sql
INSERT INTO public.items (kp_id, imdb_id, title_rus, title_orig, orig_src, created_at, updated_at)
SELECT
    source.kp_id,
    source.imdb_id,
    NULLIF(source.title_rus, ''),
    NULLIF(source.title_orig, ''),
    source.orig_src,
    now(),
    now()
FROM staging.item_import source
WHERE
    source.batch_id = $1::uuid
    AND source.import_state = 'ready'
    AND (source.kp_id IS NOT NULL OR source.imdb_id IS NOT NULL)
ON CONFLICT (kp_id) WHERE kp_id IS NOT NULL
DO UPDATE
SET
    imdb_id = COALESCE(EXCLUDED.imdb_id, items.imdb_id),
    title_rus = COALESCE(EXCLUDED.title_rus, items.title_rus),
    title_orig = COALESCE(EXCLUDED.title_orig, items.title_orig),
    updated_at = now()
WHERE
    items.deleted_at IS NULL
    AND (
        items.imdb_id IS DISTINCT FROM EXCLUDED.imdb_id
        OR items.title_rus IS DISTINCT FROM EXCLUDED.title_rus
        OR items.title_orig IS DISTINCT FROM EXCLUDED.title_orig
    )
RETURNING id, kp_id, imdb_id, created_at, updated_at;
```

## Multi-row `VALUES`

```sql
INSERT INTO match_new.model_versions (version, model_type, config, is_active, created_at)
VALUES
    (
        'ml_v1',
        'classifier',
        jsonb_build_object('threshold', 0.75, 'features', jsonb_build_array('title', 'year')),
        TRUE,
        now()
    ),
    ('rules_v2', 'rules', jsonb_build_object('strict', TRUE, 'max_year_delta', 1), FALSE, now()),
    ('manual', 'human', jsonb_build_object(), TRUE, now())
ON CONFLICT (version) DO UPDATE
SET
    config = EXCLUDED.config,
    is_active = EXCLUDED.is_active,
    updated_at = now()
RETURNING version, model_type, is_active;
```

## `UPDATE ... FROM`

```sql
UPDATE match_new.source_links link
SET
    status = CASE WHEN source.approved THEN 'approved' ELSE 'rejected' END,
    score = COALESCE(source.override_score, link.score),
    match_method = CASE WHEN source.reviewed_by IS NOT NULL THEN 'manual_review' ELSE link.match_method END,
    updated_at = now()
FROM match_new.manual_review_decisions source
LEFT JOIN auth.users reviewer ON reviewer.id = source.reviewed_by
WHERE
    source.kp_id = link.kp_id
    AND source.imdb_id = link.imdb_id
    AND source.created_at >= $1::timestamptz
    AND link.status IN ('pending', 'frozen')
    AND (reviewer.id IS NOT NULL OR source.reviewed_by IS NULL)
RETURNING link.kp_id, link.imdb_id, link.status, link.score, link.match_method;
```

## `DELETE ... USING`

```sql
DELETE FROM match_new.source_links link
USING match_new.source_links replacement, match_new.manual_entity_locks entity_lock
WHERE
    link.status = 'rejected'
    AND replacement.status = 'approved'
    AND (replacement.kp_id = link.kp_id OR replacement.imdb_id = link.imdb_id)
    AND (replacement.kp_id, replacement.imdb_id) != (link.kp_id, link.imdb_id)
    AND entity_lock.lock_state != 'active'
    AND (
        (entity_lock.entity_source = 'kp' AND entity_lock.entity_id = link.kp_id)
        OR (entity_lock.entity_source = 'imdb' AND entity_lock.entity_id = link.imdb_id)
    )
    AND link.updated_at < $1::timestamptz
RETURNING link.kp_id, link.imdb_id, link.status;
```

## `MERGE`

```sql
MERGE INTO public.items target
USING (
    SELECT source.kp_id, source.imdb_id, source.title_rus, source.title_orig, source.deleted_at, source.updated_at
    FROM staging.items source
    WHERE source.batch_id = $1::uuid AND source.validation_error IS NULL
) incoming ON target.kp_id = incoming.kp_id AND target.deleted_at IS NULL

WHEN MATCHED AND incoming.deleted_at IS NOT NULL THEN DELETE

WHEN MATCHED AND incoming.updated_at > target.updated_at THEN UPDATE SET
    imdb_id = COALESCE(incoming.imdb_id, target.imdb_id),
    title_rus = COALESCE(incoming.title_rus, target.title_rus),
    title_orig = COALESCE(incoming.title_orig, target.title_orig),
    updated_at = incoming.updated_at

WHEN NOT MATCHED AND incoming.deleted_at IS NULL THEN INSERT (kp_id, imdb_id, title_rus, title_orig, created_at, updated_at)
    VALUES (incoming.kp_id, incoming.imdb_id, incoming.title_rus, incoming.title_orig, now(), incoming.updated_at);
```

## Lateral join and logical function-argument groups

```sql
SELECT
    item.id,
    item.kp_id,
    item.imdb_id,
    COALESCE(item.title_rus, item.title_orig, '') AS title,
    activity.watch_count,
    activity.last_watched_at,
    jsonb_build_object(
        'ratings', activity.rating_count,
        'average_rating', activity.average_rating,
        'notifications', activity.notification_count
    ) AS stats
FROM public.items item
LEFT JOIN LATERAL (
    SELECT
        count(*) FILTER (WHERE event.kind = 'watch') AS watch_count,
        max(event.created_at) FILTER (WHERE event.kind = 'watch') AS last_watched_at,
        count(*) FILTER (WHERE event.kind = 'rating') AS rating_count,
        avg(event.rating) FILTER (WHERE event.kind = 'rating') AS average_rating,
        count(*) FILTER (WHERE event.kind = 'notification') AS notification_count
    FROM stats.item_activity_events event
    WHERE event.item_id = item.id AND event.created_at >= $1::timestamptz
) activity ON TRUE
WHERE
    item.deleted_at IS NULL
    AND (
        activity.watch_count > 0
        OR activity.rating_count > 0
        OR activity.notification_count > 0
    )
ORDER BY activity.last_watched_at DESC NULLS LAST, item.id;
```

## Set operations

```sql
SELECT candidate.kp_id, candidate.imdb_id, candidate.score, 'automatic' AS source
FROM match_new.match_candidates candidate
WHERE candidate.score >= $1::float8 AND candidate.model_version = $2::text

UNION ALL

SELECT decision.kp_id, decision.imdb_id, 1.0 AS score, 'manual' AS source
FROM match_new.manual_review_decisions decision
WHERE decision.decision = 'approved' AND decision.created_at >= $3::timestamptz

EXCEPT

SELECT blocked.kp_id, blocked.imdb_id, blocked.score, blocked.source
FROM match_new.blocked_candidates blocked
WHERE blocked.reason IN ('entity_lock', 'identity_conflict')
ORDER BY score DESC, kp_id, imdb_id
LIMIT $4::int;
```

## Grouping, `HAVING`, and named windows

```sql
SELECT
    date_trunc('day', session.started_at) AS day,
    item.type,
    count(*) AS sessions,
    count(DISTINCT session.user_id) AS users,
    sum(session.watched_secs) AS watched_secs,
    percentile_cont(0.5) WITHIN GROUP (ORDER BY session.watched_secs) AS median_watched_secs,
    sum(sum(session.watched_secs)) OVER monthly_window AS rolling_watched_secs
FROM stats.user_watch_sessions session
JOIN public.items item ON item.id = session.item_id
WHERE
    session.started_at >= $1::timestamptz
    AND session.started_at < $2::timestamptz
    AND item.deleted_at IS NULL
GROUP BY date_trunc('day', session.started_at), item.type
HAVING
    count(*) >= $3::bigint
    AND (
        sum(session.watched_secs) > 0
        OR count(DISTINCT session.user_id) > 1
    )
WINDOW
    base_window AS (PARTITION BY item.type ORDER BY date_trunc('day', session.started_at)),
    monthly_window AS (base_window ROWS BETWEEN 29 PRECEDING AND CURRENT ROW)
ORDER BY day, item.type;
```

## Simple and complex indexes

```sql
CREATE INDEX users_reg_date_idx ON users (reg_date);

CREATE INDEX active_users_reg_date_idx ON users (reg_date) WHERE (deleted_at IS NULL);

CREATE INDEX item_watch_sessions_daily_active_idx
    ON stats.item_watch_sessions_daily (day DESC, item_id)
    INCLUDE (watch_count, watched_secs)
    WHERE
        watch_count > 0
        OR watched_secs > 0;
```

## PL/pgSQL

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

        UPDATE match_new.source_links
        SET
            model_version = current_version,
            updated_at = now()
        WHERE model_version IS NULL AND status = 'pending';

        GET DIAGNOSTICS affected_rows = ROW_COUNT;

        IF affected_rows > 0 THEN
            INSERT INTO audit.events (event_type, payload, created_at)
            VALUES (
                'source_links_model_version_backfilled',
                jsonb_build_object('affected_rows', affected_rows, 'model_version', current_version),
                now()
            );
        END IF;

    EXCEPTION
        WHEN unique_violation THEN
            RAISE NOTICE 'audit event already exists';
    END
$$;
```
