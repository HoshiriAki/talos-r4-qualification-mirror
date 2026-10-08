-- R4-P7 session revocation invariant.
--
-- Any password_hash rotation invalidates every existing browser session for
-- that identity in the same SQLite transaction as the credential change.
-- This is a database invariant rather than a route convention.

CREATE TRIGGER IF NOT EXISTS trg_auth_password_change_revoke_sessions
AFTER UPDATE OF password_hash ON identities
FOR EACH ROW
WHEN NEW.password_hash <> OLD.password_hash
BEGIN
    DELETE FROM auth_sessions WHERE identity_id = NEW.id;
END;
