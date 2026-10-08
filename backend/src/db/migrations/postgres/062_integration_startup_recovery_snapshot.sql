-- 062: bounded startup crash-recovery snapshot for the fixture Integration worker (PG).
--
-- A snapshot is captured before this worker performs ordinary scheduling.
-- It records only durable in-flight identifiers that already existed at
-- startup; periodic ticks never use this table to recover active work.

CREATE TABLE integration_startup_recovery_snapshot (
    scheduler_id TEXT NOT NULL CHECK (scheduler_id = 'fixture_integration_worker'),
    recovery_id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    work_kind TEXT NOT NULL CHECK (work_kind IN ('external_operation', 'webhook')),
    work_id TEXT NOT NULL,
    captured_at TEXT NOT NULL,
    PRIMARY KEY (scheduler_id, recovery_id, work_kind, tenant_id, work_id)
);

CREATE INDEX idx_integration_startup_recovery_snapshot_page
    ON integration_startup_recovery_snapshot (scheduler_id, recovery_id, tenant_id);
