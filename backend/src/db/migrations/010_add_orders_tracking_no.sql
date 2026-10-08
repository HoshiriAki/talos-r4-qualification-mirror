-- Add trackingNo column to orders
ALTER TABLE orders ADD COLUMN trackingNo TEXT DEFAULT '';
