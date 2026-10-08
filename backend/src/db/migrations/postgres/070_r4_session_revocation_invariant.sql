-- R4-P7 session revocation invariant PostgreSQL parity.
--
-- Any password_hash rotation invalidates every existing browser session for
-- that identity in the same PostgreSQL transaction as the credential change.

CREATE OR REPLACE FUNCTION talos_revoke_sessions_on_password_change()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.password_hash IS DISTINCT FROM OLD.password_hash THEN
        DELETE FROM auth_sessions WHERE identity_id = NEW.id;
    END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS trg_auth_password_change_revoke_sessions ON identities;
CREATE TRIGGER trg_auth_password_change_revoke_sessions
AFTER UPDATE OF password_hash ON identities
FOR EACH ROW
EXECUTE FUNCTION talos_revoke_sessions_on_password_change();
