-- 002: Ensure devices.notes column exists (PostgreSQL)
DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM information_schema.columns
    WHERE table_name = 'devices' AND column_name = 'notes'
  ) THEN
    ALTER TABLE devices ADD COLUMN notes TEXT DEFAULT '';
  END IF;
END $$;
