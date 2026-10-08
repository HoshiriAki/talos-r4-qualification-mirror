-- Migration 045: Complete tenant scope for admin support tables (PostgreSQL).
-- Every operation is conditional because the PostgreSQL migration track does
-- not yet create all optional admin-domain tables.

DO $$
BEGIN
  IF to_regclass('public.credit_scores') IS NOT NULL THEN
    ALTER TABLE credit_scores ADD COLUMN IF NOT EXISTS tenant_id TEXT;
    UPDATE credit_scores SET tenant_id = 'default' WHERE tenant_id IS NULL;
    ALTER TABLE credit_scores ALTER COLUMN tenant_id SET NOT NULL;
    DROP INDEX IF EXISTS idx_credit_scores_phone;
    CREATE UNIQUE INDEX IF NOT EXISTS idx_credit_scores_tenant_phone
      ON credit_scores(tenant_id, customer_phone);
  END IF;
END $$;

DO $$
BEGIN
  IF to_regclass('public.order_price_details') IS NOT NULL THEN
    ALTER TABLE order_price_details ADD COLUMN IF NOT EXISTS tenant_id TEXT;

    IF to_regclass('public.orders') IS NOT NULL
       AND EXISTS (
         SELECT 1 FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name = 'orders'
           AND column_name = 'tenant_id'
       ) THEN
      UPDATE order_price_details AS details
      SET tenant_id = orders.tenant_id
      FROM orders
      WHERE details.orderId = orders.id
        AND details.tenant_id IS NULL;
    END IF;

    UPDATE order_price_details SET tenant_id = 'default' WHERE tenant_id IS NULL;
    ALTER TABLE order_price_details ALTER COLUMN tenant_id SET NOT NULL;
    CREATE INDEX IF NOT EXISTS idx_order_price_details_tenant_order
      ON order_price_details(tenant_id, orderId);
  END IF;
END $$;

CREATE OR REPLACE FUNCTION enforce_order_price_details_tenant_match()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF NEW.tenant_id IS NULL OR NOT EXISTS (
    SELECT 1 FROM orders WHERE id = NEW.orderid AND tenant_id = NEW.tenant_id
  ) THEN
    RAISE EXCEPTION 'order_price_details tenant scope must match order';
  END IF;
  RETURN NEW;
END $$;

DO $$
BEGIN
  IF to_regclass('public.order_price_details') IS NOT NULL
     AND to_regclass('public.orders') IS NOT NULL THEN
    DROP TRIGGER IF EXISTS enforce_order_price_details_tenant ON order_price_details;
    CREATE TRIGGER enforce_order_price_details_tenant
      BEFORE INSERT OR UPDATE OF tenant_id, orderid ON order_price_details
      FOR EACH ROW EXECUTE FUNCTION enforce_order_price_details_tenant_match();
  END IF;
END $$;

CREATE OR REPLACE FUNCTION enforce_overdue_notification_tenant_match()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF NEW.tenant_id IS NULL OR NOT EXISTS (
    SELECT 1 FROM overdue_records WHERE id = NEW.overdue_id AND tenant_id = NEW.tenant_id
  ) THEN
    RAISE EXCEPTION 'overdue_notification_log tenant scope must match overdue record';
  END IF;
  RETURN NEW;
END $$;

DO $$
BEGIN
  IF to_regclass('public.overdue_notification_log') IS NOT NULL
     AND to_regclass('public.overdue_records') IS NOT NULL
     AND EXISTS (SELECT 1 FROM information_schema.columns
                 WHERE table_schema = 'public'
                   AND table_name = 'overdue_notification_log'
                   AND column_name = 'tenant_id') THEN
    DROP TRIGGER IF EXISTS enforce_overdue_notification_tenant ON overdue_notification_log;
    CREATE TRIGGER enforce_overdue_notification_tenant
      BEFORE INSERT OR UPDATE OF tenant_id, overdue_id ON overdue_notification_log
      FOR EACH ROW EXECUTE FUNCTION enforce_overdue_notification_tenant_match();
  END IF;
END $$;

DO $$
BEGIN
  IF to_regclass('public.overdue_notification_log') IS NOT NULL THEN
    ALTER TABLE overdue_notification_log ADD COLUMN IF NOT EXISTS tenant_id TEXT;

    IF to_regclass('public.overdue_records') IS NOT NULL
       AND EXISTS (
         SELECT 1 FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name = 'overdue_records'
           AND column_name = 'tenant_id'
       ) THEN
      UPDATE overdue_notification_log AS notification
      SET tenant_id = overdue.tenant_id
      FROM overdue_records AS overdue
      WHERE notification.overdue_id = overdue.id
        AND notification.tenant_id IS NULL;
    END IF;

    UPDATE overdue_notification_log SET tenant_id = 'default' WHERE tenant_id IS NULL;
    ALTER TABLE overdue_notification_log ALTER COLUMN tenant_id SET NOT NULL;
    CREATE INDEX IF NOT EXISTS idx_overdue_notification_tenant_record
      ON overdue_notification_log(tenant_id, overdue_id, escalation_level);
  END IF;
END $$;

DO $$
BEGIN
  IF to_regclass('public.overdue_notification_log') IS NOT NULL
     AND to_regclass('public.overdue_records') IS NOT NULL THEN
    DROP TRIGGER IF EXISTS enforce_overdue_notification_tenant ON overdue_notification_log;
    CREATE TRIGGER enforce_overdue_notification_tenant
      BEFORE INSERT OR UPDATE OF tenant_id, overdue_id ON overdue_notification_log
      FOR EACH ROW EXECUTE FUNCTION enforce_overdue_notification_tenant_match();
  END IF;
END $$;
