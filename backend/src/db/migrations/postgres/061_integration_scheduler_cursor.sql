-- 061: persisted fair-pagination cursor for the fixture Integration worker (PG).
--
-- This is operational scheduling metadata only. It carries no tenant
-- authority and is advanced exclusively from durable Integration work that is
-- already admitted by the Store.

CREATE TABLE integration_scheduler_cursor (
    scheduler_id TEXT PRIMARY KEY CHECK (scheduler_id = 'fixture_integration_worker'),
    tenant_id TEXT,
    updated_at TEXT NOT NULL
);
