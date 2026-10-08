-- 004: Pricing config tables (PostgreSQL)

CREATE TABLE IF NOT EXISTS pricing_configs (
  id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  baseWeekdayPrice DOUBLE PRECISION NOT NULL,
  baseWeekendPrice DOUBLE PRECISION NOT NULL,
  holidayRulesJson JSONB NOT NULL DEFAULT '[]',
  updatedBy TEXT DEFAULT '',
  createdAt TEXT NOT NULL,
  updatedAt TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS dynamic_daily_prices (
  id TEXT PRIMARY KEY,
  dateKey TEXT NOT NULL UNIQUE,
  price DOUBLE PRECISION NOT NULL,
  updatedBy TEXT DEFAULT '',
  createdAt TEXT NOT NULL,
  updatedAt TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_dynamic_daily_prices_date_key ON dynamic_daily_prices(dateKey);
