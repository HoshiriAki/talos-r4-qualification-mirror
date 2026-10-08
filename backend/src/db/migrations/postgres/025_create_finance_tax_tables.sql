-- Migration 025: Finance Tax Tables
-- invoices + settlements + revenue_records + tax_config + accounting_entries

-- 发票表
CREATE TABLE IF NOT EXISTS invoices (
    id TEXT PRIMARY KEY,
    order_id TEXT NOT NULL,
    invoice_no TEXT NOT NULL UNIQUE,
    type TEXT NOT NULL CHECK(type IN ('普通发票', '专用发票')),
    amount REAL NOT NULL,
    tax_rate REAL NOT NULL DEFAULT 0.13,
    tax_amount REAL NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('issued', 'voided', 'red_flushed')),
    tenant_id TEXT NOT NULL DEFAULT '',
    issued_at TEXT NOT NULL,
    voided_at TEXT,
    created_at TEXT NOT NULL
);

-- 结算单表
CREATE TABLE IF NOT EXISTS settlements (
    id TEXT PRIMARY KEY,
    period_type TEXT NOT NULL CHECK(period_type IN ('daily', 'weekly', 'monthly')),
    period_key TEXT NOT NULL,
    total_revenue REAL NOT NULL DEFAULT 0,
    total_deposits REAL NOT NULL DEFAULT 0,
    total_refunds REAL NOT NULL DEFAULT 0,
    confirmed INTEGER NOT NULL DEFAULT 0,
    confirmed_at TEXT,
    tenant_id TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_settlements_period ON settlements(period_type, period_key, tenant_id);

-- 收入确认记录表
CREATE TABLE IF NOT EXISTS revenue_records (
    id TEXT PRIMARY KEY,
    order_id TEXT NOT NULL,
    amount REAL NOT NULL,
    recognition_date TEXT NOT NULL,
    source TEXT NOT NULL CHECK(source IN ('order_complete', 'deposit_forfeit', 'other')),
    created_at TEXT NOT NULL
);

-- 税务配置表
CREATE TABLE IF NOT EXISTS tax_config (
    id TEXT PRIMARY KEY,
    tax_type TEXT NOT NULL DEFAULT 'vat',
    rate REAL NOT NULL DEFAULT 0.13,
    effective_from TEXT NOT NULL,
    is_active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL
);

-- 会计凭证分录表
CREATE TABLE IF NOT EXISTS accounting_entries (
    id TEXT PRIMARY KEY,
    order_id TEXT NOT NULL,
    entry_type TEXT NOT NULL CHECK(entry_type IN ('debit', 'credit')),
    account TEXT NOT NULL CHECK(account IN ('revenue', 'deposit', 'receivable', 'cash')),
    amount REAL NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);

-- Seed default tax_config row (VAT 13%)
INSERT INTO tax_config (id, tax_type, rate, effective_from, is_active, created_at)
VALUES ('seed_vat_13', 'vat', 0.13, '2024-01-01', 1, '2024-01-01T00:00:00.000+08:00')
ON CONFLICT (id) DO NOTHING;
