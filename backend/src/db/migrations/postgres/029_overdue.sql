-- Migration 029: Overdue processing system 鈥?fee config, records, notification anti-duplicate log

-- Overdue fee configuration table
CREATE TABLE IF NOT EXISTS overdue_fee_config (
    id BIGSERIAL PRIMARY KEY,
    daily_rate REAL NOT NULL DEFAULT 50.0,
    max_days INTEGER NOT NULL DEFAULT 30,
    cap_multiplier REAL NOT NULL DEFAULT 3.0,
    grace_period_hours INTEGER NOT NULL DEFAULT 4,
    created_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00'),
    updated_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00')
);

-- Overdue records table
CREATE TABLE IF NOT EXISTS overdue_records (
    id BIGSERIAL PRIMARY KEY,
    order_id TEXT NOT NULL REFERENCES orders(id),
    customer_name TEXT NOT NULL,
    customer_phone TEXT NOT NULL,
    expected_return_date TEXT NOT NULL,
    actual_return_date TEXT,
    days_overdue INTEGER NOT NULL DEFAULT 0,
    daily_rate REAL NOT NULL,
    total_fee REAL NOT NULL DEFAULT 0.0,
    waived_amount REAL NOT NULL DEFAULT 0.0,
    paid_amount REAL NOT NULL DEFAULT 0.0,
    status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active', 'escalated_d1', 'escalated_d3', 'escalated_d7', 'waived', 'paid', 'cancelled')),
    escalation_level INTEGER NOT NULL DEFAULT 0,
    last_escalation_at TEXT,
    waived_by TEXT REFERENCES identities(id),
    waived_reason TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00'),
    updated_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00')
);

CREATE INDEX IF NOT EXISTS idx_overdue_order ON overdue_records(order_id);
CREATE INDEX IF NOT EXISTS idx_overdue_status ON overdue_records(status);
CREATE INDEX IF NOT EXISTS idx_overdue_phone ON overdue_records(customer_phone);

-- Overdue notification log (anti-duplicate sending)
CREATE TABLE IF NOT EXISTS overdue_notification_log (
    id BIGSERIAL PRIMARY KEY,
    overdue_id BIGINT NOT NULL REFERENCES overdue_records(id),
    escalation_level INTEGER NOT NULL,
    channel TEXT NOT NULL CHECK(channel IN ('sms', 'email', 'legal_notice')),
    sent_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00')
);

CREATE INDEX IF NOT EXISTS idx_overdue_notif ON overdue_notification_log(overdue_id, escalation_level);
