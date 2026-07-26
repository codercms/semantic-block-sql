INSERT INTO public.items (
    id, title, created_at
)
VALUES
    ($1, $2, NOW()),
    ($3, $4, NOW())
RETURNING id, created_at;
