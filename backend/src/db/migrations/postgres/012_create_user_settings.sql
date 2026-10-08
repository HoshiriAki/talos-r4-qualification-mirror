-- 012: Create user_settings table for per-account UI preferences (PostgreSQL)
CREATE TABLE IF NOT EXISTS user_settings (
  userId TEXT PRIMARY KEY,
  settingsJson JSONB NOT NULL DEFAULT '{}',
  updatedAt TEXT NOT NULL,
  FOREIGN KEY(userId) REFERENCES identities(id) ON DELETE CASCADE
);
