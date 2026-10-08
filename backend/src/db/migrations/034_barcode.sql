CREATE TABLE IF NOT EXISTS barcode_labels (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_serial_no TEXT NOT NULL UNIQUE,
    barcode_text TEXT NOT NULL UNIQUE,
    barcode_type TEXT NOT NULL DEFAULT 'CODE128' CHECK(barcode_type IN ('CODE128', 'QR', 'RFID')),
    label_format TEXT NOT NULL DEFAULT '50x25mm',
    generated_at TEXT NOT NULL DEFAULT (datetime('now', '+08:00'))
);

CREATE TABLE IF NOT EXISTS scan_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_serial_no TEXT NOT NULL,
    barcode_text TEXT,
    scan_type TEXT NOT NULL CHECK(scan_type IN ('checkout', 'checkin', 'inventory', 'transfer')),
    scanned_by TEXT NOT NULL REFERENCES identities(id),
    warehouse_id INTEGER REFERENCES warehouses(id),
    notes TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now', '+08:00'))
);
CREATE INDEX IF NOT EXISTS idx_scan_device ON scan_events(device_serial_no);
CREATE INDEX IF NOT EXISTS idx_scan_type ON scan_events(scan_type);
CREATE INDEX IF NOT EXISTS idx_scan_created ON scan_events(created_at);
