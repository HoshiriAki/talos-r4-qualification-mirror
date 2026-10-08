CREATE TABLE IF NOT EXISTS booking_availability (
    id BIGSERIAL PRIMARY KEY,
    device_serial_no TEXT NOT NULL,
    date TEXT NOT NULL,
    is_available INTEGER NOT NULL DEFAULT 1,
    reserved_order_id TEXT REFERENCES orders(id),
    created_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00'),
    UNIQUE(device_serial_no, date)
);
CREATE INDEX IF NOT EXISTS idx_booking_device_date ON booking_availability(device_serial_no, date);
