-- 024: Notification system — templates + log + seed defaults
-- event_type: shipped / return_reminder / damage_forfeit
-- channel: email / sms / in_app
-- status: pending / sent / failed

CREATE TABLE IF NOT EXISTS notification_templates (
    id TEXT PRIMARY KEY,
    event_type TEXT NOT NULL,
    channel TEXT NOT NULL,
    subject_template TEXT NOT NULL DEFAULT '',
    body_template TEXT NOT NULL DEFAULT '',
    is_enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS notification_log (
    id TEXT PRIMARY KEY,
    template_id TEXT DEFAULT '',
    event_type TEXT NOT NULL DEFAULT '',
    channel TEXT NOT NULL DEFAULT 'in_app',
    recipient TEXT NOT NULL DEFAULT '',
    subject TEXT NOT NULL DEFAULT '',
    body TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending',
    read_at TEXT DEFAULT NULL,
    error_message TEXT DEFAULT '',
    created_at TEXT NOT NULL DEFAULT ''
);

-- Seed default templates
INSERT INTO notification_templates (id, event_type, channel, subject_template, body_template, is_enabled, created_at, updated_at)
VALUES
    ('tpl_shipped_email', 'shipped', 'email', '您的 Talos 订单 {orderNo} 已发货', '尊敬的 {customerName}：\n\n您的订单 {orderNo} 已发货，设备 {deviceSerialNo} 正在运往您的途中。\n\n感谢您的耐心等待！', 1, '', ''),
    ('tpl_shipped_sms', 'shipped', 'sms', '', '【Talos租赁】您的订单 {orderNo} 已发货，设备 {deviceSerialNo} 正在派送中。', 1, '', ''),
    ('tpl_shipped_inapp', 'shipped', 'in_app', '订单已发货', '您的订单 {orderNo} 已发货，设备：{deviceSerialNo}', 1, '', ''),
    ('tpl_return_reminder_email', 'return_reminder', 'email', '您的 Talos 设备即将到期归还', '尊敬的 {customerName}：\n\n您的订单 {orderNo} 将于 {dueDate} 到期，请及时归还设备 {deviceSerialNo}。\n\n如有疑问请联系我们。', 1, '', ''),
    ('tpl_return_reminder_sms', 'return_reminder', 'sms', '', '【Talos租赁】您的订单 {orderNo} 即将于 {dueDate} 到期，请及时归还设备。', 1, '', ''),
    ('tpl_return_reminder_inapp', 'return_reminder', 'in_app', '归还提醒', '您的订单 {orderNo} 将于 {dueDate} 到期，请及时归还设备。', 1, '', ''),
    ('tpl_damage_forfeit_email', 'damage_forfeit', 'email', '设备损坏定损通知', '尊敬的 {customerName}：\n\n您的订单 {orderNo} 设备 {deviceSerialNo} 经检测存在损坏，维修费用为 ¥{amount}。\n\n该费用将从您的押金中扣除。', 1, '', ''),
    ('tpl_damage_forfeit_sms', 'damage_forfeit', 'sms', '', '【Talos租赁】您的订单 {orderNo} 设备 {deviceSerialNo} 定损费用为 ¥{amount}，将从押金中扣除。', 1, '', ''),
    ('tpl_damage_forfeit_inapp', 'damage_forfeit', 'in_app', '设备损坏定损', '订单 {orderNo} 设备 {deviceSerialNo} 定损金额：¥{amount}', 1, '', '')
ON CONFLICT(id) DO NOTHING;
