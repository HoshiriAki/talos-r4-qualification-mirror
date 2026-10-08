-- ============================================================
-- Migration 026: Asset Lifecycle Tables
-- asset_purchases — 设备采购记录
-- depreciation_log — 设备折旧日志 (直线法)
-- ============================================================

CREATE TABLE IF NOT EXISTS asset_purchases (
    id TEXT PRIMARY KEY,
    device_serial_no TEXT NOT NULL,
    purchase_price REAL NOT NULL DEFAULT 0,
    purchase_date TEXT NOT NULL DEFAULT '',
    vendor TEXT NOT NULL DEFAULT '',
    invoice_no TEXT NOT NULL DEFAULT '',
    replacement_value REAL NOT NULL DEFAULT 0,
    notes TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT '',
    UNIQUE(device_serial_no)
);

CREATE TABLE IF NOT EXISTS depreciation_log (
    id TEXT PRIMARY KEY,
    device_serial_no TEXT NOT NULL,
    period TEXT NOT NULL,
    opening_value REAL NOT NULL DEFAULT 0,
    depreciation_amount REAL NOT NULL DEFAULT 0,
    closing_value REAL NOT NULL DEFAULT 0,
    method TEXT NOT NULL DEFAULT 'straight_line',
    created_at TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_depreciation_log_device ON depreciation_log(device_serial_no);
CREATE INDEX IF NOT EXISTS idx_depreciation_log_period ON depreciation_log(period);
CREATE INDEX IF NOT EXISTS idx_asset_purchases_device ON asset_purchases(device_serial_no);
