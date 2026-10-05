SELECT COUNT(*)
FROM cdc.materialization_pending
WHERE key_bigint BETWEEN 10001 AND 10501
