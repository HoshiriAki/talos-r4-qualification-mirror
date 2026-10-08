-- R4-P8: device serial identity belongs to a tenant namespace.
-- Rebuild the legacy serial parent plus every single-column legacy child FK so
-- referential integrity follows (serialNo, tenant_id). The executor disables
-- foreign_keys before BEGIN and runs PRAGMA foreign_key_check before commit.

CREATE TABLE devices_new_073 (
    id TEXT PRIMARY KEY,
    serialNo TEXT NOT NULL,
    rentalStatus TEXT NOT NULL,
    createdAt TEXT NOT NULL,
    notes TEXT DEFAULT '',
    fallbackReturnNode TEXT DEFAULT '',
    modelId TEXT DEFAULT '',
    currentWarehouseId TEXT DEFAULT '',
    expectedWarehouseId TEXT DEFAULT '',
    expectedAvailableDate TEXT DEFAULT '',
    warning_status TEXT NOT NULL DEFAULT '正常',
    tenant_id TEXT NOT NULL,
    UNIQUE(serialNo, tenant_id)
);
INSERT INTO devices_new_073
    (id,serialNo,rentalStatus,createdAt,notes,fallbackReturnNode,modelId,
     currentWarehouseId,expectedWarehouseId,expectedAvailableDate,warning_status,tenant_id)
SELECT id,serialNo,rentalStatus,createdAt,notes,fallbackReturnNode,modelId,
       currentWarehouseId,expectedWarehouseId,expectedAvailableDate,warning_status,tenant_id
FROM devices;

CREATE TABLE order_devices_new_073 (
    id TEXT PRIMARY KEY,
    orderId TEXT NOT NULL,
    serialNo TEXT NOT NULL,
    createdAt TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    UNIQUE(orderId, serialNo),
    FOREIGN KEY(orderId) REFERENCES orders(id) ON DELETE CASCADE,
    FOREIGN KEY(serialNo, tenant_id)
        REFERENCES devices_new_073(serialNo, tenant_id) ON DELETE CASCADE
);
INSERT INTO order_devices_new_073 (id,orderId,serialNo,createdAt,tenant_id)
SELECT id,orderId,serialNo,createdAt,tenant_id FROM order_devices;

CREATE TABLE damage_reports_new_073 (
    id TEXT PRIMARY KEY,
    order_id TEXT NOT NULL,
    device_serial_no TEXT NOT NULL,
    appearance_ok INTEGER NOT NULL DEFAULT 1,
    accessories_ok INTEGER NOT NULL DEFAULT 1,
    function_ok INTEGER NOT NULL DEFAULT 1,
    damage_description TEXT NOT NULL DEFAULT '',
    estimated_damage_amount REAL NOT NULL DEFAULT 0 CHECK (estimated_damage_amount >= 0),
    liability TEXT NOT NULL DEFAULT 'unknown'
        CHECK (liability IN ('customer', 'logistics', 'warehouse', 'unknown')),
    status TEXT NOT NULL DEFAULT 'reported'
        CHECK (status IN ('reported', 'assessed', 'adjudicated')),
    reported_by TEXT NOT NULL DEFAULT '',
    assessed_by TEXT DEFAULT NULL,
    adjudicated_by TEXT DEFAULT NULL,
    reported_at TEXT NOT NULL,
    assessed_at TEXT,
    adjudicated_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    FOREIGN KEY(order_id) REFERENCES orders(id) ON DELETE CASCADE,
    FOREIGN KEY(device_serial_no, tenant_id)
        REFERENCES devices_new_073(serialNo, tenant_id) ON DELETE CASCADE
);
INSERT INTO damage_reports_new_073
    (id,order_id,device_serial_no,appearance_ok,accessories_ok,function_ok,
     damage_description,estimated_damage_amount,liability,status,reported_by,
     assessed_by,adjudicated_by,reported_at,assessed_at,adjudicated_at,
     created_at,updated_at,tenant_id)
SELECT id,order_id,device_serial_no,appearance_ok,accessories_ok,function_ok,
       damage_description,estimated_damage_amount,liability,status,reported_by,
       assessed_by,adjudicated_by,reported_at,assessed_at,adjudicated_at,
       created_at,updated_at,tenant_id
FROM damage_reports;

CREATE TABLE repair_orders_new_073 (
    id TEXT PRIMARY KEY,
    damage_report_id TEXT NOT NULL,
    device_serial_no TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'in_progress', 'completed', 'returned')),
    repair_description TEXT NOT NULL DEFAULT '',
    repair_cost REAL NOT NULL DEFAULT 0 CHECK (repair_cost >= 0),
    vendor TEXT NOT NULL DEFAULT '',
    created_by TEXT NOT NULL DEFAULT '',
    completed_by TEXT DEFAULT NULL,
    returned_by TEXT DEFAULT NULL,
    started_at TEXT,
    completed_at TEXT,
    returned_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    FOREIGN KEY(damage_report_id) REFERENCES damage_reports_new_073(id) ON DELETE CASCADE,
    FOREIGN KEY(device_serial_no, tenant_id)
        REFERENCES devices_new_073(serialNo, tenant_id) ON DELETE CASCADE
);
INSERT INTO repair_orders_new_073
    (id,damage_report_id,device_serial_no,status,repair_description,repair_cost,
     vendor,created_by,completed_by,returned_by,started_at,completed_at,returned_at,
     created_at,updated_at,tenant_id)
SELECT id,damage_report_id,device_serial_no,status,repair_description,repair_cost,
       vendor,created_by,completed_by,returned_by,started_at,completed_at,returned_at,
       created_at,updated_at,tenant_id
FROM repair_orders;

-- Cross-table triggers compiled against the canonical devices table must be
-- suspended while devices is temporarily absent. They are restored verbatim
-- after the canonical table name exists again.
DROP TRIGGER IF EXISTS prevent_tenant_delete_with_data;
DROP TRIGGER IF EXISTS trg_allocation_device_model_insert;

DROP TABLE repair_orders;
DROP TABLE damage_reports;
DROP TABLE order_devices;
DROP TABLE devices;

ALTER TABLE devices_new_073 RENAME TO devices;
ALTER TABLE order_devices_new_073 RENAME TO order_devices;
ALTER TABLE damage_reports_new_073 RENAME TO damage_reports;
ALTER TABLE repair_orders_new_073 RENAME TO repair_orders;

CREATE INDEX idx_devices_rental_status ON devices(rentalStatus);
CREATE INDEX idx_devices_tenant ON devices(tenant_id, serialNo);
CREATE INDEX idx_devices_tenant_status ON devices(tenant_id, rentalStatus);
CREATE UNIQUE INDEX idx_devices_serial_tenant_unique ON devices(serialNo, tenant_id);

CREATE INDEX idx_order_devices_serial_no ON order_devices(serialNo);
CREATE INDEX idx_order_devices_order_id ON order_devices(orderId);
CREATE INDEX idx_order_devices_tenant ON order_devices(tenant_id, orderId);

CREATE INDEX idx_damage_order ON damage_reports(order_id);
CREATE INDEX idx_damage_device ON damage_reports(device_serial_no);
CREATE INDEX idx_damage_status ON damage_reports(status);
CREATE INDEX idx_damage_reports_tenant ON damage_reports(tenant_id, device_serial_no);

CREATE INDEX idx_repair_damage ON repair_orders(damage_report_id);
CREATE INDEX idx_repair_device ON repair_orders(device_serial_no);
CREATE INDEX idx_repair_status ON repair_orders(status);
CREATE INDEX idx_repair_orders_tenant ON repair_orders(tenant_id, device_serial_no);

CREATE TRIGGER devices_tenant_id_not_null
BEFORE INSERT ON devices FOR EACH ROW WHEN NEW.tenant_id IS NULL
BEGIN SELECT RAISE(ABORT, 'tenant_id cannot be NULL in devices'); END;

CREATE TRIGGER devices_tenant_id_update_not_null
BEFORE UPDATE OF tenant_id ON devices FOR EACH ROW WHEN NEW.tenant_id IS NULL
BEGIN SELECT RAISE(ABORT, 'tenant_id cannot be NULL in devices'); END;

CREATE TRIGGER fk_devices_tenant_id_insert
BEFORE INSERT ON devices
FOR EACH ROW
BEGIN
    SELECT RAISE(ABORT, 'Foreign key constraint failed: tenant_id must reference tenants.id')
    WHERE NEW.tenant_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM tenants WHERE id = NEW.tenant_id);
END;

CREATE TRIGGER fk_devices_tenant_id_update
BEFORE UPDATE OF tenant_id ON devices
FOR EACH ROW
BEGIN
    SELECT RAISE(ABORT, 'Foreign key constraint failed: tenant_id must reference tenants.id')
    WHERE NEW.tenant_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM tenants WHERE id = NEW.tenant_id);
END;

CREATE TRIGGER prevent_tenant_delete_with_data
BEFORE DELETE ON tenants
FOR EACH ROW
BEGIN
    SELECT RAISE(ABORT, 'Cannot delete tenant: has associated orders')
    WHERE EXISTS (SELECT 1 FROM orders WHERE tenant_id = OLD.id LIMIT 1);

    SELECT RAISE(ABORT, 'Cannot delete tenant: has associated devices')
    WHERE EXISTS (SELECT 1 FROM devices WHERE tenant_id = OLD.id LIMIT 1);
END;

CREATE TRIGGER trg_allocation_device_model_insert
BEFORE INSERT ON allocations
WHEN NOT EXISTS (
    SELECT 1 FROM devices d
    WHERE d.tenant_id = NEW.tenant_id
      AND d.serialNo = NEW.device_serial_no
      AND COALESCE(d.modelId, '') = NEW.model_id
)
BEGIN
    SELECT RAISE(ABORT, 'allocation device must belong to the required model and tenant');
END;
