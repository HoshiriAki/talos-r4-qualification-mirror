-- 021: Expand orders.status from 3-state to 9-state lifecycle + status_history audit table
-- Status values: draft | confirmed | paid | shipped | in_use | returned | inspected | completed | closed | cancelled
-- Existing data migration: reserved→draft, active→in_use, completed stays, cancelled stays

-- 1. Create status_history table
CREATE TABLE IF NOT EXISTS status_history (
    id TEXT PRIMARY KEY,
    order_id TEXT NOT NULL,
    from_status TEXT NOT NULL DEFAULT '',
    to_status TEXT NOT NULL,
    operator TEXT NOT NULL DEFAULT '',
    reason TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    FOREIGN KEY (order_id) REFERENCES orders(id)
);

CREATE INDEX IF NOT EXISTS idx_status_history_order ON status_history(order_id);

-- 2. Update existing order status values to new enum
-- reserved → draft (new meaning: initial draft before staff confirmation)
UPDATE orders SET status = 'draft' WHERE status = 'reserved';
-- active → in_use (new meaning: device is in use by customer)
UPDATE orders SET status = 'in_use' WHERE status = 'active';

-- 3. Insert status_history entries for existing orders
INSERT INTO status_history (id, order_id, from_status, to_status, operator, reason, created_at)
SELECT md5(random()::text || clock_timestamp()::text), id, '', status, '',
       'migration: 021 — existing order status migrated',
       to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00'
FROM orders WHERE status IS NOT NULL AND status != '';
