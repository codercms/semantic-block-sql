WITH RECURSIVE item_tree AS (
    SELECT item.id, item.parent_id
    FROM public.items item
    WHERE item.parent_id IS NULL

    UNION ALL

    SELECT child.id, child.parent_id
    FROM public.items child
    JOIN item_tree parent ON parent.id = child.parent_id
)
SELECT item_tree.id
FROM item_tree;
