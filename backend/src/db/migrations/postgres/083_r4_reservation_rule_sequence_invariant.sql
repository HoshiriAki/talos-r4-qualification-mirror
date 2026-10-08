-- R4-P8: align the Reservation rule BIGSERIAL sequence with historical explicit seed ids.
-- Migration 032 seeded reservation_rules(id=1) explicitly, which does not advance
-- PostgreSQL's backing sequence. Tenant-scoped rule creation must therefore repair
-- the sequence before any default rule is inserted without an explicit id.

SELECT setval(
    pg_get_serial_sequence('reservation_rules', 'id'),
    COALESCE(MAX(id), 1),
    MAX(id) IS NOT NULL
)
FROM reservation_rules;
