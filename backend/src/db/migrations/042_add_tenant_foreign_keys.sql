-- Migration 042: Add foreign key constraints for tenant_id
--
-- SQLite 外键约束需要在表创建时定义，或通过重建表添加。
-- 这个迁移为关键表添加外键约束。

-- 首先确保外键支持已启用
PRAGMA foreign_keys = ON;

-- ══════════════════════════════════════════════════════════════════════════════
-- 策略：为核心表添加外键约束
-- SQLite 不支持 ALTER TABLE ADD CONSTRAINT，需要重建表
-- ══════════════════════════════════════════════════════════════════════════════

-- 注意：由于表结构复杂，我们采用触发器方式实现引用完整性检查
-- 这样可以避免重建所有表

-- 1. 创建触发器验证 orders.tenant_id 引用 tenants.id
CREATE TRIGGER fk_orders_tenant_id_insert
BEFORE INSERT ON orders
FOR EACH ROW
BEGIN
    SELECT RAISE(ABORT, 'Foreign key constraint failed: tenant_id must reference tenants.id')
    WHERE NEW.tenant_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM tenants WHERE id = NEW.tenant_id);
END;

CREATE TRIGGER fk_orders_tenant_id_update
BEFORE UPDATE OF tenant_id ON orders
FOR EACH ROW
BEGIN
    SELECT RAISE(ABORT, 'Foreign key constraint failed: tenant_id must reference tenants.id')
    WHERE NEW.tenant_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM tenants WHERE id = NEW.tenant_id);
END;

-- 2. devices 表
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

-- 3. warehouses 表
CREATE TRIGGER fk_warehouses_tenant_id_insert
BEFORE INSERT ON warehouses
FOR EACH ROW
BEGIN
    SELECT RAISE(ABORT, 'Foreign key constraint failed: tenant_id must reference tenants.id')
    WHERE NEW.tenant_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM tenants WHERE id = NEW.tenant_id);
END;

CREATE TRIGGER fk_warehouses_tenant_id_update
BEFORE UPDATE OF tenant_id ON warehouses
FOR EACH ROW
BEGIN
    SELECT RAISE(ABORT, 'Foreign key constraint failed: tenant_id must reference tenants.id')
    WHERE NEW.tenant_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM tenants WHERE id = NEW.tenant_id);
END;

-- 5. 级联删除触发器（删除租户时警告或阻止）
-- 实际实现中应该阻止删除有数据的租户
CREATE TRIGGER prevent_tenant_delete_with_data
BEFORE DELETE ON tenants
FOR EACH ROW
BEGIN
    SELECT RAISE(ABORT, 'Cannot delete tenant: has associated orders')
    WHERE EXISTS (SELECT 1 FROM orders WHERE tenant_id = OLD.id LIMIT 1);

    SELECT RAISE(ABORT, 'Cannot delete tenant: has associated devices')
    WHERE EXISTS (SELECT 1 FROM devices WHERE tenant_id = OLD.id LIMIT 1);

END;

-- Migration complete
