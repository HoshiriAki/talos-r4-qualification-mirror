-- Migration 040: Add tenant_id to remaining business tables
--
-- This migration extends tenant isolation to all business domain tables:
-- pricing, finance, damage/repair, contracts, credit, notifications, etc.

-- ── Pricing tables ──
ALTER TABLE dynamic_daily_prices ADD COLUMN tenant_id TEXT;
UPDATE dynamic_daily_prices SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_daily_prices_tenant ON dynamic_daily_prices(tenant_id, dateKey);

-- Holiday rules are JSON in pricing_configs; there is no holiday_prices table.

-- ── Finance tables ──
ALTER TABLE deposits ADD COLUMN tenant_id TEXT;
UPDATE deposits SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_deposits_tenant ON deposits(tenant_id, order_id);

ALTER TABLE refunds ADD COLUMN tenant_id TEXT;
UPDATE refunds SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_refunds_tenant ON refunds(tenant_id, deposit_id);

ALTER TABLE deposit_ledger ADD COLUMN tenant_id TEXT;
UPDATE deposit_ledger SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_deposit_ledger_tenant ON deposit_ledger(tenant_id, deposit_id);

-- invoices and settlements already include tenant_id in migration 025.

ALTER TABLE revenue_records ADD COLUMN tenant_id TEXT;
UPDATE revenue_records SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_revenue_records_tenant ON revenue_records(tenant_id, order_id);

ALTER TABLE accounting_entries ADD COLUMN tenant_id TEXT;
UPDATE accounting_entries SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_accounting_entries_tenant ON accounting_entries(tenant_id, created_at);

-- ── Damage & Repair tables ──
ALTER TABLE damage_reports ADD COLUMN tenant_id TEXT;
UPDATE damage_reports SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_damage_reports_tenant ON damage_reports(tenant_id, device_serial_no);

ALTER TABLE repair_orders ADD COLUMN tenant_id TEXT;
UPDATE repair_orders SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_repair_orders_tenant ON repair_orders(tenant_id, device_serial_no);

-- ── Asset lifecycle tables ──
ALTER TABLE asset_purchases ADD COLUMN tenant_id TEXT;
UPDATE asset_purchases SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_asset_purchases_tenant ON asset_purchases(tenant_id, device_serial_no);

ALTER TABLE depreciation_log ADD COLUMN tenant_id TEXT;
UPDATE depreciation_log SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_depreciation_log_tenant ON depreciation_log(tenant_id, device_serial_no);

-- ── Contract tables ──
ALTER TABLE contracts ADD COLUMN tenant_id TEXT;
UPDATE contracts SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_contracts_tenant ON contracts(tenant_id, order_id);

ALTER TABLE contract_templates ADD COLUMN tenant_id TEXT;
UPDATE contract_templates SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_contract_templates_tenant ON contract_templates(tenant_id, name);

ALTER TABLE e_signatures ADD COLUMN tenant_id TEXT;
UPDATE e_signatures SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_e_signatures_tenant ON e_signatures(tenant_id, contract_id);

-- ── Credit & Overdue tables ──
ALTER TABLE credit_scores ADD COLUMN tenant_id TEXT;
UPDATE credit_scores SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_credit_scores_tenant ON credit_scores(tenant_id, customer_phone);

ALTER TABLE violations ADD COLUMN tenant_id TEXT;
UPDATE violations SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_violations_tenant ON violations(tenant_id, customer_phone);

ALTER TABLE blacklist ADD COLUMN tenant_id TEXT;
UPDATE blacklist SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_blacklist_tenant ON blacklist(tenant_id, customer_phone);

ALTER TABLE overdue_records ADD COLUMN tenant_id TEXT;
UPDATE overdue_records SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_overdue_records_tenant ON overdue_records(tenant_id, order_id);

ALTER TABLE overdue_fee_config ADD COLUMN tenant_id TEXT;
UPDATE overdue_fee_config SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_overdue_fee_config_tenant ON overdue_fee_config(tenant_id);

-- ── Notification tables ──
ALTER TABLE notification_templates ADD COLUMN tenant_id TEXT;
UPDATE notification_templates SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_notification_templates_tenant ON notification_templates(tenant_id, event_type);

ALTER TABLE notification_log ADD COLUMN tenant_id TEXT;
UPDATE notification_log SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_notification_log_tenant ON notification_log(tenant_id, created_at);

-- ── Compliance tables ──
ALTER TABLE privacy_consents ADD COLUMN tenant_id TEXT;
UPDATE privacy_consents SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_privacy_consents_tenant ON privacy_consents(tenant_id, user_id);

ALTER TABLE data_deletion_requests ADD COLUMN tenant_id TEXT;
UPDATE data_deletion_requests SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_data_deletion_requests_tenant ON data_deletion_requests(tenant_id, user_id);

-- ── Optical SOP tables ──
ALTER TABLE inspection_checklists ADD COLUMN tenant_id TEXT;
UPDATE inspection_checklists SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_inspection_checklists_tenant ON inspection_checklists(tenant_id, order_id);

-- inspection_photos is not part of the current schema.

-- ── Booking & Reservation tables ──
ALTER TABLE inventory_reservations ADD COLUMN tenant_id TEXT;
UPDATE inventory_reservations SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_reservations_tenant ON inventory_reservations(tenant_id, device_serial_no);

ALTER TABLE booking_availability ADD COLUMN tenant_id TEXT;
UPDATE booking_availability SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_bookings_tenant ON booking_availability(tenant_id, device_serial_no);

-- ── Barcode tables ──
ALTER TABLE scan_events ADD COLUMN tenant_id TEXT;
UPDATE scan_events SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_scan_events_tenant ON scan_events(tenant_id, device_serial_no);

-- ── API Keys table ──
ALTER TABLE api_keys ADD COLUMN tenant_id TEXT;
UPDATE api_keys SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_api_keys_tenant ON api_keys(tenant_id, key_hash);

-- ── Work Tasks table ──
ALTER TABLE work_tasks ADD COLUMN tenant_id TEXT;
UPDATE work_tasks SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_work_tasks_tenant ON work_tasks(tenant_id, created_at);
