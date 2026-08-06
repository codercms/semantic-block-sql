SELECT
    item.id, item.title
FROM items item
JOIN stats s ON
    s.item_id = item.id
    AND s.active
-- The comment introduces the WHERE clause, not the JOIN predicate.
WHERE
    item.deleted_at IS NULL
    AND item.visible
GROUP BY
    item.id, item.title
HAVING
    COUNT(*) > 0
    AND bool_or(s.active)
ORDER BY
    item.title, item.id;

SELECT item.id
FROM items item
JOIN stats s ON s.item_id = item.id AND s.active
WHERE item.deleted_at IS NULL AND item.visible;
