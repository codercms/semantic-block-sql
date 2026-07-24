WITH active_items AS (
    SELECT item.id, item.parent_id
    FROM public.items item
    WHERE item.deleted_at IS NULL
),
root_items AS (
    SELECT active_items.id
    FROM active_items
    WHERE active_items.parent_id IS NULL
)
SELECT root_items.id
FROM root_items;
