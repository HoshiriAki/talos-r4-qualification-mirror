-- Migration 043: Add performance indexes for multi-tenant queries
--
-- NOTE: Talos uses camelCase column names (startDate, serialNo, rentalStatus, etc.)

-- ══════════════════════════════════════════════════════════════════════════════
-- Orders 表优化索引
-- ══════════════════════════════════════════════════════════════════════════════

-- 常见查询：按租户 + 状态 + 日期范围查询订单
CREATE INDEX IF NOT EXISTS idx_orders_tenant_status_startDate
    ON orders(tenant_id, status, startDate);

-- 常见查询：按租户 + 创建时间查询订单（用于仪表盘）
CREATE INDEX IF NOT EXISTS idx_orders_tenant_createdAt
    ON orders(tenant_id, createdAt);

-- ══════════════════════════════════════════════════════════════════════════════
-- Devices 表优化索引
-- ══════════════════════════════════════════════════════════════════════════════

-- 常见查询：按租户 + 状态查询可用设备
CREATE INDEX IF NOT EXISTS idx_devices_tenant_status
    ON devices(tenant_id, rentalStatus);

-- ══════════════════════════════════════════════════════════════════════════════
-- Contracts 表优化索引
-- ══════════════════════════════════════════════════════════════════════════════

CREATE INDEX IF NOT EXISTS idx_contracts_tenant_status
    ON contracts(tenant_id, status);

-- ══════════════════════════════════════════════════════════════════════════════
-- 统计查询优化
-- ══════════════════════════════════════════════════════════════════════════════

-- 订单统计：按租户 + 日期 + 状态（用于仪表盘）
CREATE INDEX IF NOT EXISTS idx_orders_tenant_createdAt_status
    ON orders(tenant_id, createdAt, status);

-- ══════════════════════════════════════════════════════════════════════════════
-- 性能说明
-- ══════════════════════════════════════════════════════════════════════════════
-- 这些索引针对以下查询模式优化：
-- 1. 租户隔离查询（WHERE tenant_id = ?）
-- 2. 租户 + 状态过滤（WHERE tenant_id = ? AND status = ?）
-- 3. 租户 + 日期范围（WHERE tenant_id = ? AND startDate BETWEEN ? AND ?）
-- 4. 租户 + 创建时间排序（ORDER BY createdAt DESC）
--
-- 索引顺序遵循：
-- - tenant_id 总是第一列（最重要的过滤条件）
-- - 等值过滤列第二（status 等）
-- - 范围过滤列最后（startDate, createdAt 等）
--
-- 预期性能提升：
-- - 小租户（<1000 记录）：影响不大，主要靠 tenant_id 索引
-- - 中等租户（1000-10000 记录）：查询速度提升 2-5x
-- - 大租户（>10000 记录）：查询速度提升 5-10x
