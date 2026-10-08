-- 019: Add structured address columns to orders (PostgreSQL)
-- Extracts province/city/district/street/detail from free-text address
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'orders' AND column_name = '_address_province') THEN
    ALTER TABLE orders ADD COLUMN _address_province TEXT DEFAULT '';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'orders' AND column_name = '_address_city') THEN
    ALTER TABLE orders ADD COLUMN _address_city TEXT DEFAULT '';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'orders' AND column_name = '_address_district') THEN
    ALTER TABLE orders ADD COLUMN _address_district TEXT DEFAULT '';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'orders' AND column_name = '_address_street') THEN
    ALTER TABLE orders ADD COLUMN _address_street TEXT DEFAULT '';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'orders' AND column_name = '_address_detail') THEN
    ALTER TABLE orders ADD COLUMN _address_detail TEXT DEFAULT '';
  END IF;
END $$;
