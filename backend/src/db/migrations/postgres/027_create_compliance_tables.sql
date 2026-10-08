-- Migration 027: Compliance tables - privacy_consents + data_deletion_requests + 2FA columns
-- Part of feature-compliance: consent tracking, data deletion requests, TOTP 2FA

-- Privacy consent records
CREATE TABLE IF NOT EXISTS privacy_consents (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    consent_type TEXT NOT NULL CHECK(consent_type IN ('privacy_policy', 'tos', 'data_processing')),
    version TEXT NOT NULL DEFAULT '1.0',
    consented INTEGER NOT NULL DEFAULT 1 CHECK(consented IN (0, 1)),
    ip_address TEXT NOT NULL DEFAULT '',
    user_agent TEXT NOT NULL DEFAULT '',
    revoked_at TEXT DEFAULT NULL,
    created_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00'),
    FOREIGN KEY (user_id) REFERENCES identities(id)
);

CREATE INDEX IF NOT EXISTS idx_privacy_consents_user_id ON privacy_consents(user_id);
CREATE INDEX IF NOT EXISTS idx_privacy_consents_type_version ON privacy_consents(consent_type, version);

-- Data deletion / anonymization requests
CREATE TABLE IF NOT EXISTS data_deletion_requests (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    request_type TEXT NOT NULL CHECK(request_type IN ('account', 'personal_data')),
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'processing', 'completed', 'rejected')),
    reason TEXT NOT NULL DEFAULT '',
    requested_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00'),
    completed_at TEXT DEFAULT NULL,
    admin_notes TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00'),
    FOREIGN KEY (user_id) REFERENCES identities(id)
);

CREATE INDEX IF NOT EXISTS idx_data_deletion_requests_user_id ON data_deletion_requests(user_id);
CREATE INDEX IF NOT EXISTS idx_data_deletion_requests_status ON data_deletion_requests(status);

-- TOTP credential state is part of identities.
