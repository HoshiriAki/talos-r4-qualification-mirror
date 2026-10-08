-- R4-P8: normalize Optical SOP identities and tenant-local referential integrity.
-- The executor disables foreign_keys around this table rebuild and runs
-- PRAGMA foreign_key_check before commit.

CREATE TABLE inspection_checklists_new_081 (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    order_id TEXT NOT NULL,
    device_serial_no TEXT NOT NULL,
    inspector_id TEXT NOT NULL,

    body_ok INTEGER NOT NULL DEFAULT 1,
    body_note TEXT,
    lens_ok INTEGER NOT NULL DEFAULT 1,
    lens_note TEXT,
    screen_ok INTEGER NOT NULL DEFAULT 1,
    screen_note TEXT,
    accessory_ok INTEGER NOT NULL DEFAULT 1,
    accessory_note TEXT,
    function_ok INTEGER NOT NULL DEFAULT 1,
    function_note TEXT,

    overall_grade TEXT NOT NULL DEFAULT 'pass'
        CHECK(overall_grade IN ('pass','minor_damage','major_damage','total_loss')),
    damage_report_id TEXT,
    photo_urls TEXT,
    notes TEXT,
    completed_at TEXT,

    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,

    UNIQUE(id, tenant_id),
    UNIQUE(tenant_id, order_id, device_serial_no),
    FOREIGN KEY(order_id, tenant_id)
        REFERENCES orders(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY(device_serial_no, tenant_id)
        REFERENCES devices(serialNo, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY(inspector_id)
        REFERENCES identities(id) ON DELETE RESTRICT,
    FOREIGN KEY(damage_report_id)
        REFERENCES damage_reports(id) ON DELETE SET NULL
);

INSERT INTO inspection_checklists_new_081 (
    id,order_id,device_serial_no,inspector_id,
    body_ok,body_note,lens_ok,lens_note,screen_ok,screen_note,
    accessory_ok,accessory_note,function_ok,function_note,
    overall_grade,damage_report_id,photo_urls,notes,completed_at,
    created_at,updated_at,tenant_id
)
SELECT
    id,CAST(order_id AS TEXT),device_serial_no,CAST(inspector_id AS TEXT),
    body_ok,body_note,lens_ok,lens_note,screen_ok,screen_note,
    accessory_ok,accessory_note,function_ok,function_note,
    overall_grade,CAST(damage_report_id AS TEXT),photo_urls,notes,NULL,
    created_at,updated_at,COALESCE(NULLIF(tenant_id,''),'default')
FROM inspection_checklists;

DROP TABLE inspection_checklists;
ALTER TABLE inspection_checklists_new_081 RENAME TO inspection_checklists;

CREATE INDEX idx_inspection_order
    ON inspection_checklists(order_id);
CREATE INDEX idx_inspection_device
    ON inspection_checklists(device_serial_no);
CREATE INDEX idx_inspection_checklists_tenant
    ON inspection_checklists(tenant_id, order_id);
CREATE INDEX idx_inspection_checklists_grade
    ON inspection_checklists(tenant_id, overall_grade);
