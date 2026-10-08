-- 008: Add accessories JSONB column to orders (PostgreSQL)
DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM information_schema.columns
    WHERE table_name = 'orders' AND column_name = 'accessories'
  ) THEN
    ALTER TABLE orders ADD COLUMN accessories JSONB DEFAULT '[]';
  END IF;
END $$;
