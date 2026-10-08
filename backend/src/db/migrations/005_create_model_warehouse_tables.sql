CREATE TABLE IF NOT EXISTS device_models (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    category TEXT NOT NULL,
    prefix TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    createdAt TEXT NOT NULL,
    updatedAt TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS model_base_prices (
    modelId TEXT PRIMARY KEY,
    weekdayPrice REAL NOT NULL,
    weekendPrice REAL NOT NULL,
    updatedBy TEXT DEFAULT '',
    createdAt TEXT NOT NULL,
    updatedAt TEXT NOT NULL,
    FOREIGN KEY(modelId) REFERENCES device_models(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS warehouses (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    type TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    createdAt TEXT NOT NULL,
    updatedAt TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS warehouse_region_rules (
    id TEXT PRIMARY KEY,
    warehouseId TEXT NOT NULL,
    province TEXT NOT NULL,
    shippingDays INTEGER NOT NULL,
    returnDays INTEGER NOT NULL,
    isPrimary INTEGER NOT NULL DEFAULT 0,
    createdAt TEXT NOT NULL,
    updatedAt TEXT NOT NULL,
    UNIQUE(warehouseId, province),
    FOREIGN KEY(warehouseId) REFERENCES warehouses(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS order_price_details (
    id TEXT PRIMARY KEY,
    orderId TEXT NOT NULL,
    dateKey TEXT NOT NULL,
    finalDailyPrice REAL NOT NULL,
    priceSource TEXT NOT NULL,
    occupyType TEXT NOT NULL,
    occupyFactor REAL NOT NULL,
    amount REAL NOT NULL,
    createdAt TEXT NOT NULL,
    UNIQUE(orderId, dateKey),
    FOREIGN KEY(orderId) REFERENCES orders(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_order_price_details_order_id ON order_price_details(orderId);
CREATE INDEX IF NOT EXISTS idx_warehouse_region_rules_warehouse ON warehouse_region_rules(warehouseId);
CREATE INDEX IF NOT EXISTS idx_warehouse_region_rules_province ON warehouse_region_rules(province);
