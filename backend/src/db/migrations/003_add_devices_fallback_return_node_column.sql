-- Conditional: only add if column doesn't exist
ALTER TABLE devices ADD COLUMN fallbackReturnNode TEXT DEFAULT '';
