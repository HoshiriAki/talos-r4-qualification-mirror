-- Migration 028: Credit system 鈥?blacklist, violations, credit_scores

-- Blacklist table
CREATE TABLE IF NOT EXISTS blacklist (
    id BIGSERIAL PRIMARY KEY,
    customer_name TEXT NOT NULL,
    customer_phone TEXT NOT NULL,
    id_number TEXT,
    reason TEXT NOT NULL,
    severity TEXT NOT NULL CHECK(severity IN ('low', 'medium', 'high', 'critical')),
    created_by TEXT NOT NULL REFERENCES identities(id),
    is_active INTEGER NOT NULL DEFAULT 1,
    removed_at TEXT,
    removed_by TEXT REFERENCES identities(id),
    removal_reason TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00'),
    updated_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00')
);

-- Violations table
CREATE TABLE IF NOT EXISTS violations (
    id BIGSERIAL PRIMARY KEY,
    customer_name TEXT NOT NULL,
    customer_phone TEXT NOT NULL,
    order_id TEXT REFERENCES orders(id),
    violation_type TEXT NOT NULL CHECK(violation_type IN ('late_return', 'damage', 'lost_device', 'fake_info', 'payment_default', 'other')),
    severity TEXT NOT NULL CHECK(severity IN ('minor', 'moderate', 'major', 'critical')),
    description TEXT NOT NULL,
    evidence TEXT,
    financial_penalty REAL DEFAULT 0.0,
    reported_by TEXT NOT NULL REFERENCES identities(id),
    status TEXT NOT NULL DEFAULT 'recorded' CHECK(status IN ('recorded', 'appealed', 'under_review', 'upheld', 'dismissed')),
    appeal_reason TEXT,
    appeal_at TEXT,
    reviewed_by TEXT REFERENCES identities(id),
    reviewed_at TEXT,
    review_notes TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00'),
    updated_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00')
);

-- Credit scores table
CREATE TABLE IF NOT EXISTS credit_scores (
    id BIGSERIAL PRIMARY KEY,
    customer_name TEXT NOT NULL,
    customer_phone TEXT NOT NULL,
    score INTEGER NOT NULL DEFAULT 100 CHECK(score >= 0 AND score <= 200),
    total_orders INTEGER NOT NULL DEFAULT 0,
    on_time_returns INTEGER NOT NULL DEFAULT 0,
    late_returns INTEGER NOT NULL DEFAULT 0,
    damage_incidents INTEGER NOT NULL DEFAULT 0,
    last_calculated_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00'),
    created_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00'),
    updated_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00')
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_blacklist_phone ON blacklist(customer_phone);
CREATE INDEX IF NOT EXISTS idx_blacklist_active ON blacklist(is_active);
CREATE INDEX IF NOT EXISTS idx_violations_phone ON violations(customer_phone);
CREATE INDEX IF NOT EXISTS idx_violations_order ON violations(order_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_credit_scores_phone ON credit_scores(customer_phone);
