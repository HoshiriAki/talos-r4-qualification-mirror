-- 010: Add trackingNo column to orders for courier tracking numbers (PostgreSQL)
-- PostgreSQL folds the unquoted trackingNo identifier to lowercase.
DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM information_schema.columns
    WHERE table_name = 'orders' AND column_name = 'trackingno'
  ) THEN
    ALTER TABLE orders ADD COLUMN trackingNo TEXT DEFAULT '';
  END IF;
END $$;
