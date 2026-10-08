-- 013_enhance_warehouses: Add metadata columns to warehouses table

ALTER TABLE warehouses ADD COLUMN address TEXT DEFAULT '';
ALTER TABLE warehouses ADD COLUMN contactName TEXT DEFAULT '';
ALTER TABLE warehouses ADD COLUMN contactPhone TEXT DEFAULT '';
ALTER TABLE warehouses ADD COLUMN notes TEXT DEFAULT '';
ALTER TABLE warehouses ADD COLUMN capacity INTEGER DEFAULT 0;
