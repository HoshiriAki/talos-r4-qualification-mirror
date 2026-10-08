-- R4-P7 platform security hardening.
--
-- Authentication throttling authority must survive process restart. The table
-- stores only a SHA-256 digest of the logical rate key; raw client IP,
-- username, identity or operation labels are not persisted here.

CREATE TABLE IF NOT EXISTS auth_rate_limit_state (
    key_hash TEXT PRIMARY KEY
        CHECK (length(key_hash) = 64),
    failure_count INTEGER NOT NULL DEFAULT 0
        CHECK (failure_count >= 0),
    window_started_at_ms INTEGER NOT NULL
        CHECK (window_started_at_ms >= 0),
    blocked_until_ms INTEGER NOT NULL DEFAULT 0
        CHECK (blocked_until_ms >= 0),
    last_seen_at_ms INTEGER NOT NULL
        CHECK (last_seen_at_ms >= 0)
);

CREATE INDEX IF NOT EXISTS idx_auth_rate_limit_last_seen
    ON auth_rate_limit_state(last_seen_at_ms);
