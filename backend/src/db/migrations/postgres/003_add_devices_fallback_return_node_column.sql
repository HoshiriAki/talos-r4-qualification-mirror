-- 003: Ensure devices.fallbackReturnNode column exists (PostgreSQL)
-- PostgreSQL folds the unquoted fallbackReturnNode identifier to lowercase.
DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM information_schema.columns
    WHERE table_name = 'devices' AND column_name = 'fallbackreturnnode'
  ) THEN
    ALTER TABLE devices ADD COLUMN fallbackReturnNode TEXT DEFAULT '';
  END IF;
END $$;
