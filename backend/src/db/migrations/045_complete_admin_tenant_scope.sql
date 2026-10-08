-- Migration 045: Complete tenant scope for admin support tables.
--
-- Parent-derived backfills preserve the owning tenant for existing rows. The
-- default tenant is only used for legacy orphan rows that predate tenant scope.

-- Credit scores used to enforce global phone uniqueness. A customer phone may
-- legitimately exist in more than one tenant, but never twice in one tenant.
DROP INDEX IF EXISTS idx_credit_scores_phone;
CREATE UNIQUE INDEX IF NOT EXISTS idx_credit_scores_tenant_phone
    ON credit_scores(tenant_id, customer_phone);

-- Price details inherit their tenant from the owning order.
ALTER TABLE order_price_details ADD COLUMN tenant_id TEXT;
UPDATE order_price_details
SET tenant_id = COALESCE(
    (SELECT orders.tenant_id
     FROM orders
     WHERE orders.id = order_price_details.orderId),
    'default'
)
WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_order_price_details_tenant_order
    ON order_price_details(tenant_id, orderId);

-- Notification attempts inherit their tenant from the overdue record.
ALTER TABLE overdue_notification_log ADD COLUMN tenant_id TEXT;
UPDATE overdue_notification_log
SET tenant_id = COALESCE(
    (SELECT overdue_records.tenant_id
     FROM overdue_records
     WHERE overdue_records.id = overdue_notification_log.overdue_id),
    'default'
)
WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_overdue_notification_tenant_record
    ON overdue_notification_log(tenant_id, overdue_id, escalation_level);

-- SQLite cannot add NOT NULL or composite foreign-key constraints in place.
-- These triggers fail closed for all new writes and tenant changes without
-- rebuilding tables that may already contain production data.
CREATE TRIGGER enforce_order_price_details_tenant_insert
BEFORE INSERT ON order_price_details
FOR EACH ROW
BEGIN
    SELECT RAISE(ABORT, 'order_price_details tenant scope is required and must match order')
    WHERE NEW.tenant_id IS NULL
       OR NOT EXISTS (
           SELECT 1 FROM orders
           WHERE orders.id = NEW.orderId
             AND orders.tenant_id = NEW.tenant_id
       );
END;

CREATE TRIGGER enforce_order_price_details_tenant_update
BEFORE UPDATE OF tenant_id, orderId ON order_price_details
FOR EACH ROW
BEGIN
    SELECT RAISE(ABORT, 'order_price_details tenant scope is required and must match order')
    WHERE NEW.tenant_id IS NULL
       OR NOT EXISTS (
           SELECT 1 FROM orders
           WHERE orders.id = NEW.orderId
             AND orders.tenant_id = NEW.tenant_id
       );
END;

CREATE TRIGGER enforce_overdue_notification_tenant_insert
BEFORE INSERT ON overdue_notification_log
FOR EACH ROW
BEGIN
    SELECT RAISE(ABORT, 'overdue_notification_log tenant scope is required and must match overdue record')
    WHERE NEW.tenant_id IS NULL
       OR NOT EXISTS (
           SELECT 1 FROM overdue_records
           WHERE overdue_records.id = NEW.overdue_id
             AND overdue_records.tenant_id = NEW.tenant_id
       );
END;

CREATE TRIGGER enforce_overdue_notification_tenant_update
BEFORE UPDATE OF tenant_id, overdue_id ON overdue_notification_log
FOR EACH ROW
BEGIN
    SELECT RAISE(ABORT, 'overdue_notification_log tenant scope is required and must match overdue record')
    WHERE NEW.tenant_id IS NULL
       OR NOT EXISTS (
           SELECT 1 FROM overdue_records
           WHERE overdue_records.id = NEW.overdue_id
             AND overdue_records.tenant_id = NEW.tenant_id
       );
END;
