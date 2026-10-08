-- 023: 设备损坏报告 + 维修工单 (SQLite)
-- damage_reports — 归还检查时登记的设备损坏
-- repair_orders  — 维修工单跟踪

CREATE TABLE IF NOT EXISTS damage_reports (
    id TEXT PRIMARY KEY,
    order_id TEXT NOT NULL,
    device_serial_no TEXT NOT NULL,
    -- 检查项: 每个 0=正常,1=损坏
    appearance_ok INTEGER NOT NULL DEFAULT 1,
    accessories_ok INTEGER NOT NULL DEFAULT 1,
    function_ok INTEGER NOT NULL DEFAULT 1,
    -- 定损
    damage_description TEXT NOT NULL DEFAULT '',
    estimated_damage_amount REAL NOT NULL DEFAULT 0 CHECK (estimated_damage_amount >= 0),
    -- 责任认定: customer|logistics|warehouse|unknown
    liability TEXT NOT NULL DEFAULT 'unknown'
        CHECK (liability IN ('customer', 'logistics', 'warehouse', 'unknown')),
    -- 状态: reported→assessed→adjudicated
    status TEXT NOT NULL DEFAULT 'reported'
        CHECK (status IN ('reported', 'assessed', 'adjudicated')),
    reported_by TEXT NOT NULL DEFAULT '',
    assessed_by TEXT DEFAULT NULL,
    adjudicated_by TEXT DEFAULT NULL,
    reported_at TEXT NOT NULL,
    assessed_at TEXT,
    adjudicated_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (order_id) REFERENCES orders(id) ON DELETE CASCADE,
    FOREIGN KEY (device_serial_no) REFERENCES devices(serialNo) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_damage_order ON damage_reports(order_id);
CREATE INDEX IF NOT EXISTS idx_damage_device ON damage_reports(device_serial_no);
CREATE INDEX IF NOT EXISTS idx_damage_status ON damage_reports(status);

CREATE TABLE IF NOT EXISTS repair_orders (
    id TEXT PRIMARY KEY,
    damage_report_id TEXT NOT NULL,
    device_serial_no TEXT NOT NULL,
    -- 维修状态: pending→in_progress→completed→returned
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'in_progress', 'completed', 'returned')),
    repair_description TEXT NOT NULL DEFAULT '',
    repair_cost REAL NOT NULL DEFAULT 0 CHECK (repair_cost >= 0),
    vendor TEXT NOT NULL DEFAULT '',
    created_by TEXT NOT NULL DEFAULT '',
    completed_by TEXT DEFAULT NULL,
    returned_by TEXT DEFAULT NULL,
    started_at TEXT,
    completed_at TEXT,
    returned_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (damage_report_id) REFERENCES damage_reports(id) ON DELETE CASCADE,
    FOREIGN KEY (device_serial_no) REFERENCES devices(serialNo) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_repair_damage ON repair_orders(damage_report_id);
CREATE INDEX IF NOT EXISTS idx_repair_device ON repair_orders(device_serial_no);
CREATE INDEX IF NOT EXISTS idx_repair_status ON repair_orders(status);
