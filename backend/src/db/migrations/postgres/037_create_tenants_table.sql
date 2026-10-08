-- Migration 037: Create tenants table for multi-tenancy support
--
-- Each tenant represents an independent rental company using the Talos platform.
-- Tenants have isolated data, separate configurations, and individual billing.

CREATE TABLE IF NOT EXISTS tenants (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  slug TEXT UNIQUE NOT NULL,           -- Subdomain identifier: acme.talos.app
  status TEXT NOT NULL DEFAULT 'active', -- active / suspended / deleted
  plan TEXT NOT NULL DEFAULT 'free',   -- free / pro / enterprise
  settings TEXT,                        -- JSON: branding, pricing policies, feature flags
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tenants_slug ON tenants(slug);
CREATE INDEX IF NOT EXISTS idx_tenants_status ON tenants(status);

-- Create default tenant for existing data
INSERT INTO tenants (id, name, slug, status, plan, created_at, updated_at)
VALUES (
  'default',
  '默认租户',
  'default',
  'active',
  'enterprise',
  to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00',
  to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00'
)
ON CONFLICT (id) DO NOTHING;
