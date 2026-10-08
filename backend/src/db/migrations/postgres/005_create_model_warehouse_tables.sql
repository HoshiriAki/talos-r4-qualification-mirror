-- 005: Device models, base prices, warehouses, region rules, order price details (PostgreSQL)

CREATE TABLE IF NOT EXISTS device_models (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  category TEXT NOT NULL,
  prefix TEXT NOT NULL,
  enabled BOOLEAN NOT NULL DEFAULT true,
  createdAt TEXT NOT NULL,
  updatedAt TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS model_base_prices (
  modelId TEXT PRIMARY KEY,
  weekdayPrice DOUBLE PRECISION NOT NULL,
  weekendPrice DOUBLE PRECISION NOT NULL,
  updatedBy TEXT DEFAULT '',
  createdAt TEXT NOT NULL,
  updatedAt TEXT NOT NULL,
  FOREIGN KEY(modelId) REFERENCES device_models(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS warehouses (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  type TEXT NOT NULL,
  enabled BOOLEAN NOT NULL DEFAULT true,
  createdAt TEXT NOT NULL,
  updatedAt TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS warehouse_region_rules (
  id TEXT PRIMARY KEY,
  warehouseId TEXT NOT NULL,
  province TEXT NOT NULL,
  shippingDays INTEGER NOT NULL,
  returnDays INTEGER NOT NULL,
  isPrimary BOOLEAN NOT NULL DEFAULT false,
  createdAt TEXT NOT NULL,
  updatedAt TEXT NOT NULL,
  UNIQUE(warehouseId, province),
  FOREIGN KEY(warehouseId) REFERENCES warehouses(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS order_price_details (
  id TEXT PRIMARY KEY,
  orderId TEXT NOT NULL,
  dateKey TEXT NOT NULL,
  finalDailyPrice DOUBLE PRECISION NOT NULL,
  priceSource TEXT NOT NULL,
  occupyType TEXT NOT NULL,
  occupyFactor DOUBLE PRECISION NOT NULL,
  amount DOUBLE PRECISION NOT NULL,
  createdAt TEXT NOT NULL,
  UNIQUE(orderId, dateKey),
  FOREIGN KEY(orderId) REFERENCES orders(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_order_price_details_order_id ON order_price_details(orderId);
CREATE INDEX IF NOT EXISTS idx_warehouse_region_rules_warehouse ON warehouse_region_rules(warehouseId);
CREATE INDEX IF NOT EXISTS idx_warehouse_region_rules_province ON warehouse_region_rules(province);

-- Conditional column additions. All identifiers below are intentionally unquoted,
-- so PostgreSQL stores and reports them in lowercase through information_schema.
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'devices' AND column_name = 'modelid') THEN
    ALTER TABLE devices ADD COLUMN modelId TEXT DEFAULT '';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'devices' AND column_name = 'currentwarehouseid') THEN
    ALTER TABLE devices ADD COLUMN currentWarehouseId TEXT DEFAULT '';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'devices' AND column_name = 'expectedwarehouseid') THEN
    ALTER TABLE devices ADD COLUMN expectedWarehouseId TEXT DEFAULT '';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'devices' AND column_name = 'expectedavailabledate') THEN
    ALTER TABLE devices ADD COLUMN expectedAvailableDate TEXT DEFAULT '';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'orders' AND column_name = 'totalprice') THEN
    ALTER TABLE orders ADD COLUMN totalPrice DOUBLE PRECISION DEFAULT 0;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'orders' AND column_name = 'province') THEN
    ALTER TABLE orders ADD COLUMN province TEXT DEFAULT '';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'orders' AND column_name = 'sendwarehouseid') THEN
    ALTER TABLE orders ADD COLUMN sendWarehouseId TEXT DEFAULT '';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'orders' AND column_name = 'returnwarehouseid') THEN
    ALTER TABLE orders ADD COLUMN returnWarehouseId TEXT DEFAULT '';
  END IF;
END $$;
