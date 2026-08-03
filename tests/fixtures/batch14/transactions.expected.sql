-- Subject update and queue completion must be in one transaction.
-- $1 sku_id, $2 claim_token, $3 applied_amount
BEGIN;
SELECT sku_id, applied_amount FROM subject_updates WHERE claim_token = $2;
COMMIT;

-- Serializable worker transaction.
BEGIN TRANSACTION ISOLATION LEVEL SERIALIZABLE, READ WRITE, NOT DEFERRABLE;
SELECT sku_id FROM subject_updates WHERE claim_token = $2;
COMMIT WORK;

BEGIN WORK ISOLATION LEVEL REPEATABLE READ, READ ONLY, DEFERRABLE;
COMMIT TRANSACTION;

BEGIN ISOLATION LEVEL READ COMMITTED;
COMMIT;

BEGIN ISOLATION LEVEL READ UNCOMMITTED;
COMMIT AND NO CHAIN;
