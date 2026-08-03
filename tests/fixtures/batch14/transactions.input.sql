-- Subject update and queue completion must be in one transaction.
-- $1 sku_id, $2 claim_token, $3 applied_amount
begin;
select sku_id,applied_amount from subject_updates where claim_token=$2;
commit;

-- Serializable worker transaction.
begin transaction isolation level serializable,read write,not deferrable;
select sku_id from subject_updates where claim_token=$2;
commit work;

begin work isolation level repeatable read,read only,deferrable;
commit transaction;

begin isolation level read committed;
commit;

begin isolation level read uncommitted;
commit and no chain;
