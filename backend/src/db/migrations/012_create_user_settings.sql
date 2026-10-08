CREATE TABLE IF NOT EXISTS user_settings (
    userId TEXT PRIMARY KEY,
    settingsJson TEXT NOT NULL DEFAULT '{}',
    updatedAt TEXT NOT NULL,
    FOREIGN KEY(userId) REFERENCES identities(id) ON DELETE CASCADE
);
