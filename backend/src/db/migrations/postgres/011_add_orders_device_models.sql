-- 011: Add deviceModels JSONB column to orders for storing selected model counts (PostgreSQL)
-- PostgreSQL folds the unquoted deviceModels identifier to lowercase.
DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM information_schema.columns
    WHERE table_name = 'orders' AND column_name = 'devicemodels'
  ) THEN
    ALTER TABLE orders ADD COLUMN deviceModels JSONB DEFAULT '{}';
  END IF;
END $$;
