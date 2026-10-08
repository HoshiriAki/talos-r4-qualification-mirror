pub mod txt {
    pub const PICKUP_SF: &str = "顺丰标快";
    pub const PICKUP_SELF: &str = "自取/跑腿";
    pub const PICKUP_HALF_DAY: &str = "半日达";
    pub const STATUS_CHECKED_IN: &str = "已入库";
    pub const STATUS_RENTING: &str = "租赁中";
    pub const STATUS_REPAIR: &str = "返厂维修";
    pub const STATUS_LOST: &str = "确认丢失";
    pub const STATUS_SCRAPPED: &str = "已报废";
    pub const STATUS_RESERVED: &str = "预约";
    pub const STATUS_ACTIVE: &str = "进行中";
    pub const STATUS_COMPLETED: &str = "已完成";
    pub const WARNING_NORMAL: &str = "正常";
    pub const WARNING_LOST: &str = "疑似丢失";
    // 021 — 9 态订单状态机
    pub const STATUS_DRAFT: &str = "草稿";
    pub const STATUS_CONFIRMED: &str = "已确认";
    pub const STATUS_PAID: &str = "已付款";
    pub const STATUS_SHIPPED: &str = "已发货";
    pub const STATUS_IN_USE: &str = "使用中";
    pub const STATUS_RETURNED: &str = "已归还";
    pub const STATUS_INSPECTED: &str = "检查中";
    pub const STATUS_CLOSED: &str = "已关闭";
    pub const STATUS_CANCELLED: &str = "已取消";
}

pub const PICKUP_METHODS: &[&str] = &[txt::PICKUP_SF, txt::PICKUP_SELF, txt::PICKUP_HALF_DAY];
pub const DEVICE_STATUS: &[&str] = &[
    txt::STATUS_CHECKED_IN,
    txt::STATUS_RENTING,
    txt::STATUS_REPAIR,
    txt::STATUS_LOST,
    txt::STATUS_SCRAPPED,
];
pub const ORDER_STATUS: &[&str] = &[
    txt::STATUS_DRAFT,
    txt::STATUS_CONFIRMED,
    txt::STATUS_PAID,
    txt::STATUS_SHIPPED,
    txt::STATUS_IN_USE,
    txt::STATUS_RETURNED,
    txt::STATUS_INSPECTED,
    txt::STATUS_COMPLETED,
    txt::STATUS_CLOSED,
    txt::STATUS_CANCELLED,
];
pub const ORDER_STATUS_DB_VALUES: &[&str] = &[
    "draft",
    "confirmed",
    "paid",
    "shipped",
    "in_use",
    "returned",
    "inspected",
    "completed",
    "closed",
    "cancelled",
];
pub const STOCK_STATUSES: &[&str] = &[txt::STATUS_CHECKED_IN];
