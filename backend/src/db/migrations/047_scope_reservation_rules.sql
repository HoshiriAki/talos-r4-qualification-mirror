-- MVP0: reservation policies and records are tenant-owned.
ALTER TABLE reservation_rules ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'default';
CREATE UNIQUE INDEX IF NOT EXISTS idx_reservation_rules_tenant
    ON reservation_rules(tenant_id);

UPDATE inventory_reservations SET tenant_id = 'default' WHERE tenant_id IS NULL;
CREATE TRIGGER IF NOT EXISTS inventory_reservations_tenant_required_insert
BEFORE INSERT ON inventory_reservations
FOR EACH ROW WHEN NEW.tenant_id IS NULL OR trim(NEW.tenant_id) = ''
BEGIN
    SELECT RAISE(ABORT, 'inventory_reservations.tenant_id is required');
END;
CREATE TRIGGER IF NOT EXISTS inventory_reservations_tenant_required_update
BEFORE UPDATE OF tenant_id ON inventory_reservations
FOR EACH ROW WHEN NEW.tenant_id IS NULL OR trim(NEW.tenant_id) = ''
BEGIN
    SELECT RAISE(ABORT, 'inventory_reservations.tenant_id is required');
END;
