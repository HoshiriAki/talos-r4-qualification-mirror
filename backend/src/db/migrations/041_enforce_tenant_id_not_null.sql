-- Pre-Alpha clean-database contract: all rows have already been backfilled by 038/040.
-- SQLite cannot add NOT NULL to existing columns, so enforce the invariant at writes.

CREATE TRIGGER orders_tenant_id_not_null
BEFORE INSERT ON orders FOR EACH ROW WHEN NEW.tenant_id IS NULL
BEGIN SELECT RAISE(ABORT, 'tenant_id cannot be NULL in orders'); END;

CREATE TRIGGER orders_tenant_id_update_not_null
BEFORE UPDATE OF tenant_id ON orders FOR EACH ROW WHEN NEW.tenant_id IS NULL
BEGIN SELECT RAISE(ABORT, 'tenant_id cannot be NULL in orders'); END;

CREATE TRIGGER devices_tenant_id_not_null
BEFORE INSERT ON devices FOR EACH ROW WHEN NEW.tenant_id IS NULL
BEGIN SELECT RAISE(ABORT, 'tenant_id cannot be NULL in devices'); END;

CREATE TRIGGER devices_tenant_id_update_not_null
BEFORE UPDATE OF tenant_id ON devices FOR EACH ROW WHEN NEW.tenant_id IS NULL
BEGIN SELECT RAISE(ABORT, 'tenant_id cannot be NULL in devices'); END;

CREATE TRIGGER warehouses_tenant_id_not_null
BEFORE INSERT ON warehouses FOR EACH ROW WHEN NEW.tenant_id IS NULL
BEGIN SELECT RAISE(ABORT, 'tenant_id cannot be NULL in warehouses'); END;

CREATE TRIGGER warehouses_tenant_id_update_not_null
BEFORE UPDATE OF tenant_id ON warehouses FOR EACH ROW WHEN NEW.tenant_id IS NULL
BEGIN SELECT RAISE(ABORT, 'tenant_id cannot be NULL in warehouses'); END;

CREATE TRIGGER device_models_tenant_id_not_null
BEFORE INSERT ON device_models FOR EACH ROW WHEN NEW.tenant_id IS NULL
BEGIN SELECT RAISE(ABORT, 'tenant_id cannot be NULL in device_models'); END;

CREATE TRIGGER device_models_tenant_id_update_not_null
BEFORE UPDATE OF tenant_id ON device_models FOR EACH ROW WHEN NEW.tenant_id IS NULL
BEGIN SELECT RAISE(ABORT, 'tenant_id cannot be NULL in device_models'); END;

CREATE TRIGGER contracts_tenant_id_not_null
BEFORE INSERT ON contracts FOR EACH ROW WHEN NEW.tenant_id IS NULL
BEGIN SELECT RAISE(ABORT, 'tenant_id cannot be NULL in contracts'); END;

CREATE TRIGGER contracts_tenant_id_update_not_null
BEFORE UPDATE OF tenant_id ON contracts FOR EACH ROW WHEN NEW.tenant_id IS NULL
BEGIN SELECT RAISE(ABORT, 'tenant_id cannot be NULL in contracts'); END;
