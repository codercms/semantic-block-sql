-- selected identity
select
    item.id, -- stable id
    /* external ids */
    item.kp_id, item.imdb_id
from public.items item;
