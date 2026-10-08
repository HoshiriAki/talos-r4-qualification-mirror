-- MVP4: canonical pricing document revision source for the simulation overlay.
-- The normal pricing module remains unaware of simulation; revision sync is
-- digest-based at simulation boundaries and therefore fail-closed.
CREATE TABLE IF NOT EXISTS simulation_pricing_resource_revisions (
    tenant_id TEXT NOT NULL,
    resource_key TEXT NOT NULL CHECK(resource_key = 'default'),
    present INTEGER NOT NULL CHECK(present IN (0,1)),
    resource_version INTEGER NOT NULL CHECK(resource_version > 0),
    document_json TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY(tenant_id, resource_key)
);
