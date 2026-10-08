CREATE TABLE rental_returns (
    id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    order_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('receiving', 'received')),
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    received_at TEXT,
    PRIMARY KEY (tenant_id, id),
    UNIQUE (tenant_id, order_id)
);

CREATE TABLE rental_return_items (
    id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    return_id TEXT NOT NULL,
    allocation_id TEXT NOT NULL,
    device_serial_no TEXT NOT NULL,
    receive_identity TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status = 'received'),
    received_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id),
    UNIQUE (tenant_id, allocation_id),
    UNIQUE (tenant_id, receive_identity),
    FOREIGN KEY (tenant_id, return_id) REFERENCES rental_returns(tenant_id, id)
);

CREATE TABLE rental_inspections (
    id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    return_item_id TEXT NOT NULL,
    allocation_id TEXT NOT NULL,
    device_serial_no TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'in_progress', 'passed', 'failed')),
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
    started_at TEXT,
    completed_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id),
    UNIQUE (tenant_id, allocation_id),
    FOREIGN KEY (tenant_id, return_item_id) REFERENCES rental_return_items(tenant_id, id)
);

CREATE TABLE rental_damage_reviews (
    id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    inspection_id TEXT NOT NULL,
    order_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('open', 'reviewed')),
    resolution_note TEXT,
    created_at TEXT NOT NULL,
    resolved_at TEXT,
    PRIMARY KEY (tenant_id, id),
    UNIQUE (tenant_id, inspection_id),
    FOREIGN KEY (tenant_id, inspection_id) REFERENCES rental_inspections(tenant_id, id)
);

CREATE TABLE rental_settlements (
    id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    order_id TEXT NOT NULL,
    currency TEXT NOT NULL CHECK (length(currency) = 3),
    amount_minor INTEGER NOT NULL,
    facts_hash TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('blocked', 'calculated', 'terminal')),
    blocker_code TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id),
    UNIQUE (tenant_id, order_id)
);

CREATE INDEX idx_return_items_return ON rental_return_items(tenant_id, return_id);
CREATE INDEX idx_inspections_status ON rental_inspections(tenant_id, status);
CREATE INDEX idx_damage_reviews_order ON rental_damage_reviews(tenant_id, order_id, status);
CREATE INDEX idx_settlements_status ON rental_settlements(tenant_id, status);
