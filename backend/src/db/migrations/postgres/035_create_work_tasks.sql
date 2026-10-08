CREATE TABLE IF NOT EXISTS work_tasks (
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
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_work_tasks_status ON work_tasks(status);
CREATE INDEX IF NOT EXISTS idx_work_tasks_assignee ON work_tasks(assignee_id);
