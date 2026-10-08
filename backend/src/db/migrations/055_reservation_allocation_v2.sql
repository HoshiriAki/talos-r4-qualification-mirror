-- Migration 055: R1-P4 Reservation / Allocation V2.
-- Canonical reservations use UUID text identifiers, half-open [start_date, end_date)
-- intervals, model-capacity requirements, explicit device allocations and migration
-- exceptions for ambiguous legacy Booking/Reservation/order_devices records.

CREATE UNIQUE INDEX IF NOT EXISTS idx_device_models_id_tenant_unique
    ON device_models(id, tenant_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_devices_serial_tenant_unique
    ON devices(serialNo, tenant_id);

CREATE TABLE IF NOT EXISTS rental_reservations (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    order_id TEXT,
    source_kind TEXT NOT NULL DEFAULT 'order'
        CHECK(source_kind IN ('order', 'legacy_booking')),
    status TEXT NOT NULL DEFAULT 'hold'
        CHECK(status IN ('hold', 'confirmed', 'released', 'expired')),
    start_date TEXT NOT NULL,
    end_date TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1 CHECK(version >= 1),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK(start_date < end_date),
    UNIQUE(id, tenant_id),
    FOREIGN KEY(order_id, tenant_id)
        REFERENCES orders(id, tenant_id) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS reservation_requirements (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    reservation_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    quantity INTEGER NOT NULL CHECK(quantity > 0),
    created_at TEXT NOT NULL,
    UNIQUE(tenant_id, reservation_id, model_id),
    FOREIGN KEY(reservation_id, tenant_id)
        REFERENCES rental_reservations(id, tenant_id) ON DELETE CASCADE,
    FOREIGN KEY(model_id, tenant_id)
        REFERENCES device_models(id, tenant_id) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS allocations (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    reservation_id TEXT NOT NULL,
    order_id TEXT,
    device_serial_no TEXT NOT NULL,
    model_id TEXT NOT NULL,
    start_date TEXT NOT NULL,
    end_date TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'allocated'
        CHECK(status IN ('allocated', 'released')),
    allocated_at TEXT NOT NULL,
    released_at TEXT,
    UNIQUE(id, tenant_id),
    FOREIGN KEY(reservation_id, tenant_id)
        REFERENCES rental_reservations(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY(order_id, tenant_id)
        REFERENCES orders(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY(device_serial_no, tenant_id)
        REFERENCES devices(serialNo, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY(model_id, tenant_id)
        REFERENCES device_models(id, tenant_id) ON DELETE RESTRICT,
    CHECK(start_date < end_date)
);

CREATE TABLE IF NOT EXISTS reservation_migration_exceptions (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    source_table TEXT NOT NULL
        CHECK(source_table IN ('inventory_reservations', 'booking_availability', 'order_devices')),
    source_id TEXT NOT NULL,
    reason TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK(status IN ('pending', 'resolved', 'ignored')),
    canonical_reservation_id TEXT,
    created_at TEXT NOT NULL,
    resolved_at TEXT,
    UNIQUE(tenant_id, source_table, source_id),
    FOREIGN KEY(canonical_reservation_id, tenant_id)
        REFERENCES rental_reservations(id, tenant_id) ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS idx_reservation_window
    ON rental_reservations(tenant_id, status, start_date, end_date);
CREATE INDEX IF NOT EXISTS idx_reservation_expiry
    ON rental_reservations(tenant_id, status, expires_at);
CREATE INDEX IF NOT EXISTS idx_reservation_order
    ON rental_reservations(tenant_id, order_id);
CREATE INDEX IF NOT EXISTS idx_reservation_requirement_model
    ON reservation_requirements(tenant_id, model_id, reservation_id);
CREATE INDEX IF NOT EXISTS idx_allocations_device_window
    ON allocations(tenant_id, device_serial_no, status, start_date, end_date);
CREATE INDEX IF NOT EXISTS idx_allocations_reservation_model
    ON allocations(tenant_id, reservation_id, model_id, status);
CREATE INDEX IF NOT EXISTS idx_reservation_migration_pending
    ON reservation_migration_exceptions(tenant_id, status, source_table);

-- Device allocation is only legal for a confirmed canonical reservation and its exact
-- half-open interval. This also prevents a route/client from changing interval semantics.
CREATE TRIGGER IF NOT EXISTS trg_allocation_reservation_contract_insert
BEFORE INSERT ON allocations
WHEN NOT EXISTS (
    SELECT 1 FROM rental_reservations r
    WHERE r.id = NEW.reservation_id
      AND r.tenant_id = NEW.tenant_id
      AND r.status = 'confirmed'
      AND r.start_date = NEW.start_date
      AND r.end_date = NEW.end_date
      AND (r.order_id IS NEW.order_id OR r.order_id = NEW.order_id)
)
BEGIN
    SELECT RAISE(ABORT, 'allocation must match one confirmed reservation interval/order');
END;

CREATE TRIGGER IF NOT EXISTS trg_allocation_device_model_insert
BEFORE INSERT ON allocations
WHEN NOT EXISTS (
    SELECT 1 FROM devices d
    WHERE d.tenant_id = NEW.tenant_id
      AND d.serialNo = NEW.device_serial_no
      AND COALESCE(d.modelId, '') = NEW.model_id
)
BEGIN
    SELECT RAISE(ABORT, 'allocation device must belong to the required model and tenant');
END;

-- Half-open overlap: [a,b) conflicts with [c,d) iff a < d AND c < b.
CREATE TRIGGER IF NOT EXISTS trg_allocation_no_overlap_insert
BEFORE INSERT ON allocations
WHEN NEW.status = 'allocated' AND EXISTS (
    SELECT 1 FROM allocations a
    WHERE a.tenant_id = NEW.tenant_id
      AND a.device_serial_no = NEW.device_serial_no
      AND a.status = 'allocated'
      AND NEW.start_date < a.end_date
      AND a.start_date < NEW.end_date
)
BEGIN
    SELECT RAISE(ABORT, 'device already allocated in overlapping half-open interval');
END;

CREATE TRIGGER IF NOT EXISTS trg_allocation_requirement_capacity_insert
BEFORE INSERT ON allocations
WHEN NEW.status = 'allocated' AND (
    NOT EXISTS (
        SELECT 1 FROM reservation_requirements rr
        WHERE rr.tenant_id = NEW.tenant_id
          AND rr.reservation_id = NEW.reservation_id
          AND rr.model_id = NEW.model_id
    )
    OR (
        SELECT COUNT(*) FROM allocations a
        WHERE a.tenant_id = NEW.tenant_id
          AND a.reservation_id = NEW.reservation_id
          AND a.model_id = NEW.model_id
          AND a.status = 'allocated'
    ) >= (
        SELECT rr.quantity FROM reservation_requirements rr
        WHERE rr.tenant_id = NEW.tenant_id
          AND rr.reservation_id = NEW.reservation_id
          AND rr.model_id = NEW.model_id
    )
)
BEGIN
    SELECT RAISE(ABORT, 'allocation exceeds reservation model requirement');
END;

-- Legacy records are staged, never silently promoted into the new UUID authority.
INSERT OR IGNORE INTO reservation_migration_exceptions
    (id, tenant_id, source_table, source_id, reason, created_at)
SELECT
    'inventory_reservations:' || CAST(id AS TEXT),
    tenant_id,
    'inventory_reservations',
    CAST(id AS TEXT),
    'legacy reservation uses integer identity/free-text customer/specific-device semantics',
    datetime('now')
FROM inventory_reservations;

INSERT OR IGNORE INTO reservation_migration_exceptions
    (id, tenant_id, source_table, source_id, reason, created_at)
SELECT
    'booking_availability:' || CAST(id AS TEXT),
    tenant_id,
    'booking_availability',
    CAST(id AS TEXT),
    'legacy per-day booking occupancy requires half-open interval reconstruction',
    datetime('now')
FROM booking_availability
WHERE is_available = 0;

INSERT OR IGNORE INTO reservation_migration_exceptions
    (id, tenant_id, source_table, source_id, reason, created_at)
SELECT
    'order_devices:' || id,
    tenant_id,
    'order_devices',
    id,
    'legacy order-device relation requires explicit Reservation/Allocation reconstruction',
    datetime('now')
FROM order_devices;
