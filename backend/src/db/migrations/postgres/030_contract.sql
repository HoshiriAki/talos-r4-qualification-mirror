-- Migration 030: Contracts + e-signatures 鈥?rental contract system with template management and digital signatures

-- Contract templates table
CREATE TABLE IF NOT EXISTS contract_templates (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    content_json TEXT NOT NULL DEFAULT '{}',
    is_active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00')
);

-- Contracts table
CREATE TABLE IF NOT EXISTS contracts (
    id BIGSERIAL PRIMARY KEY,
    order_id TEXT NOT NULL REFERENCES orders(id),
    template_id BIGINT REFERENCES contract_templates(id),
    customer_name TEXT NOT NULL,
    customer_phone TEXT NOT NULL,
    device_value REAL NOT NULL DEFAULT 0.0,
    content_json TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'draft' CHECK(status IN ('draft', 'generated', 'signed', 'verified', 'expired', 'voided')),
    signed_at TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00'),
    updated_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00')
);

CREATE INDEX IF NOT EXISTS idx_contracts_order ON contracts(order_id);
CREATE INDEX IF NOT EXISTS idx_contracts_status ON contracts(status);
CREATE INDEX IF NOT EXISTS idx_contracts_phone ON contracts(customer_phone);

-- E-signatures table
CREATE TABLE IF NOT EXISTS e_signatures (
    id BIGSERIAL PRIMARY KEY,
    contract_id BIGINT NOT NULL REFERENCES contracts(id),
    signer_name TEXT NOT NULL,
    signer_phone TEXT NOT NULL,
    signature_data TEXT NOT NULL,
    signed_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00')
);

CREATE INDEX IF NOT EXISTS idx_esign_contract ON e_signatures(contract_id);

-- Seed a default rental contract template
INSERT INTO contract_templates (id, name, content_json, is_active) VALUES (
    1,
    '鏍囧噯璁惧绉熻祦鍚堝悓',
    '{"sections":[{"title":"鐢叉柟鍚戜箼鏂瑰嚭绉熻澶?,"body":"鐢叉柟锛堝嚭绉熸柟锛夊悜涔欐柟锛堟壙绉熸柟锛夊嚭绉?DJI Pocket 3 鐩告満璁惧涓€濂楋紝鍏蜂綋瑙勬牸鍙婃暟閲忚瑙佽澶囨竻鍗曘€?},{"title":"绉熻祦鏈熼檺","body":"绉熻祦鏈熼檺鑷?{{rentalStart}} 璧疯嚦 {{rentalEnd}} 姝紝鍏辫 {{totalDays}} 澶┿€?},{"title":"绉熼噾鍙婃娂閲?,"body":"绉熼噾鎬婚涓轰汉姘戝竵 {{totalPrice}} 鍏冦€傛娂閲戜负浜烘皯甯?{{depositAmount}} 鍏冿紝璁惧褰掕繕妫€鏌ユ棤璇悗閫€杩樸€?},{"title":"璁惧浠峰€?,"body":"绉熻祦璁惧鎬讳环鍊间负浜烘皯甯?{{deviceValue}} 鍏冦€傚鍙戠敓鎹熷潖鎴栦涪澶憋紝涔欐柟闇€鎸夊疄闄呬环鍊艰禂鍋裤€?},{"title":"鐢叉柟璐ｄ换","body":"1. 鐢叉柟淇濊瘉鍑虹璁惧瀹屽ソ鍙敤銆俓\n2. 鐢叉柟璐熻矗璁惧鐨勬棩甯告妧鏈敮鎸併€?},{"title":"涔欐柟璐ｄ换","body":"1. 涔欐柟搴斿Ε鍠勪繚绠¤澶囷紝涓嶅緱杞銆俓\n2. 涔欐柟搴旀寜鏈熷綊杩樿澶囷紝閫炬湡鎸夋瘡鏃?{{overdueRate}} 鍏冩敹鍙栨粸绾抽噾銆俓\n3. 璁惧濡傛湁鎹熷潖锛屼箼鏂瑰簲鍙婃椂鍛婄煡鐢叉柟銆?},{"title":"杩濈害璐ｄ换","body":"浠讳綍涓€鏂硅繚鍙嶆湰鍚堝悓绾﹀畾锛屽簲璧斿伩瀵规柟鍥犳閬彈鐨勫叏閮ㄦ崯澶便€?},{"title":"浜夎瑙ｅ喅","body":"鏈悎鍚屽湪灞ヨ杩囩▼涓彂鐢熺殑浜夎锛岀敱鍙屾柟鍗忓晢瑙ｅ喅銆?},{"title":"绛剧讲","body":"鏈悎鍚屼竴寮忎袱浠斤紝鐢蹭箼鍙屾柟鍚勬墽涓€浠斤紝鍏锋湁鍚岀瓑娉曞緥鏁堝姏銆俓\n\\n鐢叉柟绛剧珷锛歘___________  涔欐柟绛剧珷锛歘___________\\n鏃ユ湡锛歿{signDate}}  鏃ユ湡锛歿{signDate}}"}],"variables":["rentalStart","rentalEnd","totalDays","totalPrice","depositAmount","deviceValue","overdueRate","signDate"]}',
    1
) ON CONFLICT (id) DO NOTHING;
