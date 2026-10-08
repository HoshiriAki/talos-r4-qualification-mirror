-- 017: Extend device statuses with lost/scrapped states (PostgreSQL)
-- rentalStatus now supports: 已入库, 租赁中, 返厂维修, 确认丢失, 已报废
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'devices' AND column_name = 'warning_status') THEN
    ALTER TABLE devices ADD COLUMN warning_status TEXT NOT NULL DEFAULT '正常';
  END IF;
END $$;
