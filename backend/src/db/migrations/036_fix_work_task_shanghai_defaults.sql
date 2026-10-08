-- Rebuild work_tasks so fresh timestamps are generated in Asia/Shanghai,
-- including databases where migration 035 was already recorded as applied.
BEGIN IMMEDIATE;

ALTER TABLE work_tasks RENAME TO work_tasks_legacy_036;

CREATE TABLE work_tasks (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL DEFAULT 'general',
  status TEXT NOT NULL DEFAULT 'queued',
  risk TEXT NOT NULL DEFAULT 'medium',
  source_type TEXT NOT NULL DEFAULT 'system',
  source_id TEXT NOT NULL DEFAULT '',
  title TEXT NOT NULL DEFAULT '',
  summary TEXT NOT NULL DEFAULT '',
  reason TEXT NOT NULL DEFAULT '',
  due_at TEXT,
  capabilities_json TEXT NOT NULL DEFAULT '[]',
  work_route_json TEXT NOT NULL DEFAULT '{"name":"dashboard"}',
  assignee_id TEXT,
  version INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+08:00', 'now', '+8 hours')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+08:00', 'now', '+8 hours'))
);

INSERT INTO work_tasks (
  id, kind, status, risk, source_type, source_id, title, summary, reason,
  due_at, capabilities_json, work_route_json, assignee_id, version,
  created_at, updated_at
)
SELECT
  id, kind, status, risk, source_type, source_id, title, summary, reason,
  due_at, capabilities_json, work_route_json, assignee_id, version,
  CASE WHEN instr(created_at, 'T') = 0
    THEN strftime('%Y-%m-%dT%H:%M:%S+08:00', created_at, '+8 hours')
    ELSE created_at END,
  CASE WHEN instr(updated_at, 'T') = 0
    THEN strftime('%Y-%m-%dT%H:%M:%S+08:00', updated_at, '+8 hours')
    ELSE updated_at END
FROM work_tasks_legacy_036;

DROP TABLE work_tasks_legacy_036;
CREATE INDEX idx_work_tasks_status ON work_tasks(status);
CREATE INDEX idx_work_tasks_assignee ON work_tasks(assignee_id);

COMMIT;
