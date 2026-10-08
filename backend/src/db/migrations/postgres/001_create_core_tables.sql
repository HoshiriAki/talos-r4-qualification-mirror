-- 001: Core business tables (PostgreSQL)
-- Talos rental system foundation schema

CREATE TABLE IF NOT EXISTS orders (
  id TEXT PRIMARY KEY,
  orderNo TEXT NOT NULL UNIQUE,
  startDate TEXT NOT NULL,
  endDate TEXT NOT NULL,
  deliveryDate TEXT NOT NULL,
  pickupMethods TEXT NOT NULL,
  address TEXT DEFAULT '',
  notes TEXT DEFAULT '',
  deviceSerialNo TEXT DEFAULT '',
  createdAt TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS devices (
  id TEXT PRIMARY KEY,
  serialNo TEXT NOT NULL UNIQUE,
  rentalStatus TEXT NOT NULL,
  createdAt TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS order_devices (
  id TEXT PRIMARY KEY,
  orderId TEXT NOT NULL,
  serialNo TEXT NOT NULL,
  createdAt TEXT NOT NULL,
  UNIQUE(orderId, serialNo),
  FOREIGN KEY(orderId) REFERENCES orders(id) ON DELETE CASCADE,
  FOREIGN KEY(serialNo) REFERENCES devices(serialNo) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS identities (
  id TEXT PRIMARY KEY,
  username TEXT UNIQUE NOT NULL,
  email TEXT UNIQUE,
  password_hash TEXT NOT NULL,
  display_name TEXT NOT NULL,
  phone TEXT,
  totp_secret_ciphertext TEXT,
  totp_enabled BOOLEAN NOT NULL DEFAULT FALSE,
  status TEXT NOT NULL CHECK (status IN ('active', 'disabled', 'locked')),
  last_login_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS auth_sessions (
  id TEXT PRIMARY KEY,
  token_hash TEXT NOT NULL UNIQUE,
  identity_id TEXT NOT NULL,
  auth_strength TEXT NOT NULL DEFAULT 'password' CHECK (auth_strength IN ('password', 'mfa')),
  created_at TEXT NOT NULL,
  last_seen_at TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  revoked_at TEXT,
  FOREIGN KEY(identity_id) REFERENCES identities(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS audit_logs (
  id TEXT PRIMARY KEY,
  actorIdentityId TEXT DEFAULT '',
  actorUsername TEXT DEFAULT '',
  actionType TEXT NOT NULL,
  entityType TEXT NOT NULL,
  entityId TEXT DEFAULT '',
  entityLabel TEXT DEFAULT '',
  detailJson JSONB DEFAULT '{}',
  ip TEXT DEFAULT '',
  userAgent TEXT DEFAULT '',
  createdAt TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_audit_logs_created_at ON audit_logs(createdAt DESC);
CREATE INDEX IF NOT EXISTS idx_audit_logs_action_type ON audit_logs(actionType);
CREATE INDEX IF NOT EXISTS idx_audit_logs_entity_type ON audit_logs(entityType);
CREATE INDEX IF NOT EXISTS idx_audit_logs_actor_username ON audit_logs(actorUsername);
