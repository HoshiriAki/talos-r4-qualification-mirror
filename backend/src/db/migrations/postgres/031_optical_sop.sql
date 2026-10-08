-- Migration 031: Optical inspection SOP 鈥?5-step return checklists + grading

CREATE TABLE IF NOT EXISTS inspection_checklists (
    id BIGSERIAL PRIMARY KEY,
    order_id TEXT NOT NULL REFERENCES orders(id),
    device_serial_no TEXT NOT NULL,
    inspector_id TEXT NOT NULL REFERENCES identities(id),

    -- 5-step checks
    body_ok INTEGER NOT NULL DEFAULT 1,        -- Step 1: body inspection
    body_note TEXT,
    lens_ok INTEGER NOT NULL DEFAULT 1,        -- Step 2: lens inspection
    lens_note TEXT,
    screen_ok INTEGER NOT NULL DEFAULT 1,      -- Step 3: screen inspection
    screen_note TEXT,
    accessory_ok INTEGER NOT NULL DEFAULT 1,   -- Step 4: accessory inspection
    accessory_note TEXT,
    function_ok INTEGER NOT NULL DEFAULT 1,    -- Step 5: functional inspection
    function_note TEXT,

    overall_grade TEXT NOT NULL DEFAULT 'pass' CHECK(overall_grade IN ('pass', 'minor_damage', 'major_damage', 'total_loss')),
    damage_report_id TEXT REFERENCES damage_reports(id),
    photo_urls TEXT,  -- JSON array of image URLs
    notes TEXT,

    created_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00'),
    updated_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00')
);

CREATE INDEX IF NOT EXISTS idx_inspection_order ON inspection_checklists(order_id);
CREATE INDEX IF NOT EXISTS idx_inspection_device ON inspection_checklists(device_serial_no);
