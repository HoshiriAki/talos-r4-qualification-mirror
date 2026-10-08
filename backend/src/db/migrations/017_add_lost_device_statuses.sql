-- 017: Extend device statuses with lost/scrapped states
-- rentalStatus now supports: 已入库, 租赁中, 返厂维修, 确认丢失, 已报废
-- Existing data unchanged. The warning_status column tracks computed vs confirmed warnings.
ALTER TABLE devices ADD COLUMN warning_status TEXT NOT NULL DEFAULT '正常';
