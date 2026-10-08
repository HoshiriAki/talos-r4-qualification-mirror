//! 租户上下文 — 多租户数据隔离基础设施。
//!
//! 当前（v0）所有订单属于内部操作，tenant 为 None。
//! 未来（v1+）客户自助下单时，通过此结构进行数据隔离。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantContext {
    pub tenant_type: TenantType,
    /// 客户 ID（TenantType::Customer 时必填）
    pub tenant_id: Option<String>,
    /// 数据可见范围
    pub scope: DataScope,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TenantType {
    /// 员工 — 可切换视角查看所有客户数据
    Staff,
    /// 客户 — 数据自动限定为 Owned
    Customer,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DataScope {
    /// 仅自己的数据
    Owned,
    /// 全部数据（admin）
    All,
    /// 指定仓库范围
    Warehouse(String),
}
