CREATE TABLE IF NOT EXISTS pricing_configs (
    id INTEGER PRIMARY KEY,
    baseWeekdayPrice REAL NOT NULL,
    baseWeekendPrice REAL NOT NULL,
    holidayRulesJson TEXT NOT NULL DEFAULT '[]',
    updatedBy TEXT DEFAULT '',
    createdAt TEXT NOT NULL,
    updatedAt TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS dynamic_daily_prices (
    id TEXT PRIMARY KEY,
    dateKey TEXT NOT NULL UNIQUE,
    price REAL NOT NULL,
    updatedBy TEXT DEFAULT '',
    createdAt TEXT NOT NULL,
    updatedAt TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_dynamic_daily_prices_date_key ON dynamic_daily_prices(dateKey);
