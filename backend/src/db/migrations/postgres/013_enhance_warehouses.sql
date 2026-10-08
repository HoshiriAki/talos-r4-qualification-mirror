-- 013: Add address, contactName, contactPhone, notes, capacity to warehouses (PostgreSQL)
-- PostgreSQL folds the unquoted contactName/contactPhone identifiers to lowercase.
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'warehouses' AND column_name = 'address') THEN
    ALTER TABLE warehouses ADD COLUMN address TEXT DEFAULT '';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'warehouses' AND column_name = 'contactname') THEN
    ALTER TABLE warehouses ADD COLUMN contactName TEXT DEFAULT '';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'warehouses' AND column_name = 'contactphone') THEN
    ALTER TABLE warehouses ADD COLUMN contactPhone TEXT DEFAULT '';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'warehouses' AND column_name = 'notes') THEN
    ALTER TABLE warehouses ADD COLUMN notes TEXT DEFAULT '';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'warehouses' AND column_name = 'capacity') THEN
    ALTER TABLE warehouses ADD COLUMN capacity INTEGER DEFAULT 0;
  END IF;
END $$;
