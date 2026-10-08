-- Migration 030: Contracts + e-signatures — rental contract system with template management and digital signatures

-- Contract templates table
CREATE TABLE IF NOT EXISTS contract_templates (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    content_json TEXT NOT NULL DEFAULT '{}',
    is_active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (datetime('now', '+08:00'))
);

-- Contracts table
CREATE TABLE IF NOT EXISTS contracts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    order_id INTEGER NOT NULL REFERENCES orders(id),
    template_id INTEGER REFERENCES contract_templates(id),
    customer_name TEXT NOT NULL,
    customer_phone TEXT NOT NULL,
    device_value REAL NOT NULL DEFAULT 0.0,
    content_json TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'draft' CHECK(status IN ('draft', 'generated', 'signed', 'verified', 'expired', 'voided')),
    signed_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now', '+08:00')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now', '+08:00'))
);

CREATE INDEX IF NOT EXISTS idx_contracts_order ON contracts(order_id);
CREATE INDEX IF NOT EXISTS idx_contracts_status ON contracts(status);
CREATE INDEX IF NOT EXISTS idx_contracts_phone ON contracts(customer_phone);

-- E-signatures table
CREATE TABLE IF NOT EXISTS e_signatures (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    contract_id INTEGER NOT NULL REFERENCES contracts(id),
    signer_name TEXT NOT NULL,
    signer_phone TEXT NOT NULL,
    signature_data TEXT NOT NULL,
    signed_at TEXT NOT NULL DEFAULT (datetime('now', '+08:00'))
);

CREATE INDEX IF NOT EXISTS idx_esign_contract ON e_signatures(contract_id);

-- Seed a default rental contract template
INSERT OR IGNORE INTO contract_templates (id, name, content_json, is_active) VALUES (
    1,
    '标准设备租赁合同',
    '{"sections":[{"title":"甲方向乙方出租设备","body":"甲方（出租方）向乙方（承租方）出租 DJI Pocket 3 相机设备一套，具体规格及数量详见设备清单。"},{"title":"租赁期限","body":"租赁期限自 {{rentalStart}} 起至 {{rentalEnd}} 止，共计 {{totalDays}} 天。"},{"title":"租金及押金","body":"租金总额为人民币 {{totalPrice}} 元。押金为人民币 {{depositAmount}} 元，设备归还检查无误后退还。"},{"title":"设备价值","body":"租赁设备总价值为人民币 {{deviceValue}} 元。如发生损坏或丢失，乙方需按实际价值赔偿。"},{"title":"甲方责任","body":"1. 甲方保证出租设备完好可用。\\n2. 甲方负责设备的日常技术支持。"},{"title":"乙方责任","body":"1. 乙方应妥善保管设备，不得转租。\\n2. 乙方应按期归还设备，逾期按每日 {{overdueRate}} 元收取滞纳金。\\n3. 设备如有损坏，乙方应及时告知甲方。"},{"title":"违约责任","body":"任何一方违反本合同约定，应赔偿对方因此遭受的全部损失。"},{"title":"争议解决","body":"本合同在履行过程中发生的争议，由双方协商解决。"},{"title":"签署","body":"本合同一式两份，甲乙双方各执一份，具有同等法律效力。\\n\\n甲方签章：____________  乙方签章：____________\\n日期：{{signDate}}  日期：{{signDate}}"}],"variables":["rentalStart","rentalEnd","totalDays","totalPrice","depositAmount","deviceValue","overdueRate","signDate"]}',
    1
);
