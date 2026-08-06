CREATE OR REPLACE FUNCTION get_user_watch_notifications_new_titles(userIds bigint[], globalLastIdSeq uuid = NULL)
RETURNS TABLE (
    user_id bigint,
    id_seq uuid,
    item_id bigint,
    TYPE user_watch_notification_type,
    reason user_watch_notification_reason,
    rel_item_id bigint,
    rel_type related_item_rel_type,
    user_list_type user_stats.list_type
) LANGUAGE SQL
BEGIN ATOMIC
    -- Freeze the upper availability bound so every cursor group sees the same batch.
    WITH max_av_dt AS (
        (SELECT id_seq FROM item_availability_dates WHERE globalLastIdSeq IS NULL ORDER BY id_seq DESC LIMIT 1)

        UNION ALL

        (
            SELECT globalLastIdSeq WHERE globalLastIdSeq IS NOT NULL
        )
    ),
    -- Users sharing a cursor can scan the same availability range together.
    cursors AS (
        SELECT array_agg(user_id) AS user_ids, last_av_dt_id, row_number() OVER () AS rn
        FROM (
            SELECT user_id, last_av_dt_id
            FROM user_stats.watch_notifications_cursor
            WHERE
                user_id = ANY (userIds)
                AND last_av_dt_id < (
                    SELECT id_seq
                    FROM max_av_dt
                )
            ORDER BY last_av_dt_id, user_id
        ) s
        GROUP BY last_av_dt_id
    ),
    -- Expand cursor groups into per-user titles that became available in this batch.
    pre_agg AS NOT MATERIALIZED (
        SELECT c_uid, av_dt.item_id, av_dt.id_seq
        FROM cursors
        CROSS JOIN unnest(user_ids) c_uid
        JOIN item_availability_dates av_dt ON
            av_dt.id_seq > cursors.last_av_dt_id
            AND av_dt.id_seq <= (
                SELECT id_seq
                FROM max_av_dt
            )
    ),
    -- New titles related to something the user has watched.
    user_new_related_items_by_watch AS (
        SELECT DISTINCT ON
            (c_uid, pre_agg.item_id)
            c_uid,
            pre_agg.id_seq,
            rel.item_id AS ul_item_id,
            rel.rel_item_id AS new_item_id,
            rel.rel_type,
            NULL::user_stats.list_type AS list_type
        FROM pre_agg
        JOIN item_related_items rel ON
            rel.rel_item_id = pre_agg.item_id
            AND rel.rel_type IN ('sequel', 'prequel', 'spin_off')
        JOIN user_stats.watch_progress uw ON
            uw.user_id = c_uid
            AND uw.item_id = rel.item_id
        -- Filter the originating title before DISTINCT/deduplication so another
        -- unmuted relation to the same new title can still produce a notification.
        WHERE
            NOT EXISTS (
                SELECT 1
                FROM user_stats.watch_notification_mutes mute
                WHERE mute.user_id = c_uid AND mute.item_id = rel.item_id
            )
            -- Do not notify about a related title the user has already watched.
            AND NOT EXISTS (
                SELECT 1
                FROM user_stats.watch_progress uw
                WHERE uw.user_id = c_uid AND uw.item_id = rel.rel_item_id
            )
    ),
    -- New titles related to something in an active user list.
    user_new_related_items_by_lists AS (
        SELECT DISTINCT ON
            (c_uid, pre_agg.item_id)
            c_uid,
            pre_agg.id_seq,
            rel.item_id AS ul_item_id,
            rel.rel_item_id AS new_item_id,
            rel.rel_type,
            ul.list_type
        FROM pre_agg
        JOIN item_related_items rel ON
            rel.rel_item_id = pre_agg.item_id
            AND rel.rel_type IN ('sequel', 'prequel', 'spin_off')
        JOIN user_stats.watch_lists ul ON
            ul.user_id = c_uid
            AND ul.item_id = rel.item_id
            AND ul.list_type != 'abandoned'
        -- The relation source, not the newly available title, owns this mute.
        WHERE
            NOT EXISTS (
                SELECT 1
                FROM user_stats.watch_notification_mutes mute
                WHERE mute.user_id = c_uid AND mute.item_id = rel.item_id
            )
            -- Do not notify about a related title the user has already watched.
            AND NOT EXISTS (
                SELECT 1
                FROM user_stats.watch_progress uw
                WHERE uw.user_id = c_uid AND uw.item_id = rel.rel_item_id
            )
    ),
    -- Newly available titles already present in an active user list.
    user_new_items_in_planning AS (
        SELECT DISTINCT ON
            (c_uid, pre_agg.item_id)
            c_uid,
            pre_agg.id_seq,
            ul.item_id AS ul_item_id,
            ul.item_id AS new_item_id,
            NULL::related_item_rel_type AS rel_type,
            ul.list_type
        FROM pre_agg
        JOIN user_stats.watch_lists ul ON
            ul.user_id = c_uid
            AND ul.item_id = pre_agg.item_id
            AND ul.list_type NOT IN ('abandoned', 'watched')
        -- Direct-list notifications originate from the newly available/listed title itself.
        WHERE
            NOT EXISTS (
                SELECT 1
                FROM user_stats.watch_notification_mutes mute
                WHERE mute.user_id = c_uid AND mute.item_id = ul.item_id
            )
            AND NOT EXISTS (
                SELECT 1
                FROM user_stats.watch_progress uw
                WHERE uw.user_id = c_uid AND uw.item_id = ul.item_id
            )
    ),
    -- Deduplicate only after each source path has applied its own mute policy.
    combined AS (
        SELECT DISTINCT ON (c_uid, new_item_id) *
        FROM (
            SELECT 'by_watch_rel'::user_watch_notification_reason AS src, * FROM user_new_related_items_by_watch

            UNION ALL

            SELECT 'by_list_rel'::user_watch_notification_reason AS src, * FROM user_new_related_items_by_lists

            UNION ALL

            SELECT 'by_list'::user_watch_notification_reason AS src, * FROM user_new_items_in_planning
        ) s
    )
    SELECT
        c_uid,
        id_seq,
        new_item_id AS item_id,
        'new_item'::user_watch_notification_type,
        src AS reason,
        CASE WHEN src IN ('by_watch_rel', 'by_list_rel') THEN ul_item_id END,
        rel_type,
        list_type AS user_list_type
    FROM combined
    ORDER BY c_uid, id_seq;
END;

CREATE OR REPLACE FUNCTION get_user_watch_notifications_new_episodes(userIds bigint[], globalLastIdSeq uuid = NULL)
RETURNS TABLE (
    user_id bigint,
    id_seq uuid,
    item_id bigint,
    season int,
    episode int,
    TYPE user_watch_notification_type,
    reason user_watch_notification_reason,
    user_list_type user_stats.list_type
) LANGUAGE SQL STRICT
BEGIN ATOMIC
    -- Freeze the upper episode-availability bound for this generation batch.
    WITH max_av_dt AS (
        (SELECT id_seq FROM item_episode_availability_dates WHERE globalLastIdSeq IS NULL ORDER BY id_seq DESC LIMIT 1)

        UNION ALL

        (
            SELECT globalLastIdSeq WHERE globalLastIdSeq IS NOT NULL
        )
    ),
    -- Group users by cursor to share availability-range scans.
    cursors AS (
        SELECT array_agg(user_id) AS user_ids, last_av_ep_dt_id, row_number() OVER () AS rn
        FROM (
            SELECT user_id, last_av_ep_dt_id
            FROM user_stats.watch_notifications_cursor
            WHERE
                user_id = ANY (userIds)
                AND last_av_ep_dt_id < (
                    SELECT id_seq
                    FROM max_av_dt
                )
            ORDER BY last_av_ep_dt_id, user_id
        ) s
        GROUP BY last_av_ep_dt_id
    ),
    -- Resolve newly available source episodes to MDB episode IDs when possible.
    pre_agg AS NOT MATERIALIZED (
        SELECT
            c_uid,
            av_dt.id_seq,
            av_dt.item_id,
            av_dt.season,
            av_dt.episode,
            ie.id AS episode_id
        FROM cursors
        CROSS JOIN unnest(user_ids) c_uid
        JOIN item_episode_availability_dates av_dt ON
            av_dt.id_seq > cursors.last_av_ep_dt_id
            AND av_dt.id_seq <= (
                SELECT id_seq
                FROM max_av_dt
            )
        LEFT JOIN item_episodes ie ON
            ie.item_id = av_dt.item_id
            AND ie.season = av_dt.season
            AND ie.episode = av_dt.episode
    ),
    -- Episode notifications for titles with watch progress.
    user_new_episodes_by_watch AS (
        SELECT DISTINCT ON
            (c_uid, pre_agg.id_seq)
            c_uid,
            pre_agg.id_seq,
            pre_agg.item_id,
            pre_agg.season,
            pre_agg.episode,
            NULL::user_stats.list_type AS user_list
        FROM pre_agg
        JOIN user_stats.watch_progress uw ON
            uw.user_id = c_uid
            AND uw.item_id = pre_agg.item_id
        -- Episode notifications originate from their parent title; filter before deduplication.
        WHERE
            NOT EXISTS (
                SELECT 1
                FROM user_stats.watch_notification_mutes mute
                WHERE mute.user_id = c_uid AND mute.item_id = pre_agg.item_id
            )
            -- Suppress episodes already present in per-episode watch progress.
            AND NOT EXISTS (
                SELECT 1
                FROM user_stats.watch_progress_per_episode usw
                WHERE
                    usw.user_id = c_uid
                    AND usw.item_id = pre_agg.item_id
                    AND usw.episode_id = pre_agg.episode_id
            )
    ),
    -- Episode notifications for titles in active user lists.
    user_new_episodes_by_list AS (
        SELECT DISTINCT ON
            (c_uid, pre_agg.id_seq)
            c_uid,
            pre_agg.id_seq,
            pre_agg.item_id,
            pre_agg.season,
            pre_agg.episode,
            ul.list_type AS user_list
        FROM pre_agg
        JOIN user_stats.watch_lists ul ON
            ul.user_id = c_uid
            AND ul.item_id = pre_agg.item_id
            AND ul.list_type NOT IN ('abandoned', 'watched')
        -- Episode notifications originate from their parent title; filter before deduplication.
        WHERE
            NOT EXISTS (
                SELECT 1
                FROM user_stats.watch_notification_mutes mute
                WHERE mute.user_id = c_uid AND mute.item_id = pre_agg.item_id
            )
            -- Suppress episodes already present in per-episode watch progress.
            AND NOT EXISTS (
                SELECT 1
                FROM user_stats.watch_progress_per_episode usw
                WHERE
                    usw.user_id = c_uid
                    AND usw.item_id = pre_agg.item_id
                    AND usw.episode_id = pre_agg.episode_id
            )
    ),
    -- Collapse watch/list eligibility to one notification per user and episode event.
    combined AS (
        SELECT DISTINCT ON (c_uid, id_seq) *
        FROM (
            SELECT 'by_watch'::user_watch_notification_reason AS src, * FROM user_new_episodes_by_watch

            UNION ALL

            SELECT 'by_list'::user_watch_notification_reason AS src, * FROM user_new_episodes_by_list
        ) s
        ORDER BY c_uid, id_seq, src
    )
    SELECT
        c_uid,
        id_seq,
        item_id,
        season,
        episode,
        'new_episode'::user_watch_notification_type,
        src AS reason,
        user_list AS user_list_type
    FROM combined;
END;
