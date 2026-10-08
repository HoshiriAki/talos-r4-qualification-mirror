-- Conditional: only add if column doesn't exist
-- Rust layer checks PRAGMA table_info before running
ALTER TABLE devices ADD COLUMN notes TEXT DEFAULT '';
