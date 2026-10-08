-- 022: 押金 + 退款 基础表 (SQLite)
-- deposits — 押金主表
-- refunds  — 退款审批表
-- deposit_ledger — 押金流水 (复式记账视角)

CREATE TABLE IF NOT EXISTS deposits (
    id TEXT PRIMARY KEY,
    order_id TEXT NOT NULL,
    amount REAL NOT NULL CHECK (amount >= 0),
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'paid', 'released', 'forfeited', 'partially_forfeited')),
    paid_at TEXT,
    released_at TEXT,
    forfeited_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (order_id) REFERENCES orders(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_deposits_order ON deposits(order_id);
CREATE INDEX IF NOT EXISTS idx_deposits_status ON deposits(status);

CREATE TABLE IF NOT EXISTS refunds (
    id TEXT PRIMARY KEY,
    deposit_id TEXT NOT NULL,
    order_id TEXT NOT NULL,
    amount REAL NOT NULL CHECK (amount > 0),
    reason TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'approved', 'rejected', 'executed')),
    requested_by TEXT NOT NULL DEFAULT '',
    approved_by TEXT DEFAULT NULL,
    rejected_by TEXT DEFAULT NULL,
    executed_by TEXT DEFAULT NULL,
    approved_at TEXT,
    rejected_at TEXT,
    executed_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (deposit_id) REFERENCES deposits(id) ON DELETE CASCADE,
    FOREIGN KEY (order_id) REFERENCES orders(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_refunds_deposit ON refunds(deposit_id);
CREATE INDEX IF NOT EXISTS idx_refunds_order ON refunds(order_id);
CREATE INDEX IF NOT EXISTS idx_refunds_status ON refunds(status);

CREATE TABLE IF NOT EXISTS deposit_ledger (
    id TEXT PRIMARY KEY,
    deposit_id TEXT NOT NULL,
    order_id TEXT NOT NULL,
    entry_type TEXT NOT NULL
        CHECK (entry_type IN ('collect', 'release', 'forfeit', 'refund')),
    amount REAL NOT NULL,
    balance_after REAL NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    operator TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    FOREIGN KEY (deposit_id) REFERENCES deposits(id) ON DELETE CASCADE,
    FOREIGN KEY (order_id) REFERENCES orders(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_ledger_deposit ON deposit_ledger(deposit_id);
CREATE INDEX IF NOT EXISTS idx_ledger_order ON deposit_ledger(order_id);
