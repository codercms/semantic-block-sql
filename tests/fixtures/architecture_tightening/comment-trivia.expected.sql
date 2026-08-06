SELECT id
FROM items
WHERE
    active
    -- The following branch is intentionally documented.
    AND NOT EXISTS (
        SELECT 1
        FROM hidden
        WHERE hidden.item_id = items.id
    );
