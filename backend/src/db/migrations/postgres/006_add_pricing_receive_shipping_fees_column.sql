-- 006: Add configurable receive-area shipping fees column (PostgreSQL)
-- PostgreSQL folds the unquoted receiveShippingFeesJson identifier to lowercase.
DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM information_schema.columns
    WHERE table_name = 'pricing_configs' AND column_name = 'receiveshippingfeesjson'
  ) THEN
    ALTER TABLE pricing_configs
    ADD COLUMN receiveShippingFeesJson JSONB NOT NULL DEFAULT '{"area1":7,"area4":18,"area2":7,"area3":7}';
  END IF;
END $$;
