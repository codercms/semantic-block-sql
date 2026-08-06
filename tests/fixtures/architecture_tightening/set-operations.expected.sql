WITH active_rows AS (
    (
        SELECT id FROM active_items
    )

    UNION ALL

    (
        SELECT id FROM archived_items
    )
),
combined_rows AS (
    SELECT id
    FROM active_rows
)
SELECT id
FROM (
    SELECT id FROM active_rows

    UNION ALL

    -- The comment belongs to the following branch.
    SELECT id FROM combined_rows
) combined;
