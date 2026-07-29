CREATE OR REPLACE FUNCTION normalize_item(item_id bigint)
RETURNS bigint
LANGUAGE plpgsql
AS $$
DECLARE
    result_id bigint := item_id;
BEGIN
    IF result_id > 0 THEN
        RETURN result_id;
    ELSE
        RETURN 0;
    END IF;
EXCEPTION
    WHEN unique_violation THEN
        RAISE NOTICE 'duplicate';

    WHEN OTHERS THEN
        RETURN -1;
END;
$$;
