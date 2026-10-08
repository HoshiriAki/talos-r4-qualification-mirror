use serde::Serialize;
use std::collections::HashMap;

// ── Logistics matrix ───────────────────────────────────────────

/// Hardcoded SF Express transit estimates for all 31 mainland China provinces
/// to 3 warehouses (上海仓, 珠海仓, 南昌友仓).
///
/// Shipping = days from warehouse to province (for delivery)
/// Return = days from province back to warehouse

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogisticsEstimate {
    pub shipping_days: i64,
    pub return_days: i64,
}

fn build_logistics_matrix() -> HashMap<&'static str, HashMap<&'static str, (i64, i64)>> {
    let mut matrix = HashMap::new();

    // 上海仓 (owned) — faster for eastern/central provinces
    let mut shanghai = HashMap::new();
    shanghai.insert("上海", (1, 1));
    shanghai.insert("江苏", (1, 1));
    shanghai.insert("浙江", (1, 1));
    shanghai.insert("安徽", (1, 2));
    shanghai.insert("北京", (2, 2));
    shanghai.insert("天津", (2, 2));
    shanghai.insert("河北", (2, 2));
    shanghai.insert("山东", (2, 2));
    shanghai.insert("河南", (2, 2));
    shanghai.insert("山西", (2, 3));
    shanghai.insert("湖北", (2, 2));
    shanghai.insert("湖南", (2, 3));
    shanghai.insert("江西", (1, 2));
    shanghai.insert("福建", (2, 2));
    shanghai.insert("广东", (2, 3));
    shanghai.insert("广西", (3, 3));
    shanghai.insert("海南", (3, 4));
    shanghai.insert("辽宁", (2, 3));
    shanghai.insert("吉林", (3, 3));
    shanghai.insert("黑龙江", (3, 4));
    shanghai.insert("内蒙古", (3, 4));
    shanghai.insert("陕西", (2, 3));
    shanghai.insert("甘肃", (3, 4));
    shanghai.insert("宁夏", (3, 4));
    shanghai.insert("青海", (4, 5));
    shanghai.insert("新疆", (4, 5));
    shanghai.insert("西藏", (4, 5));
    shanghai.insert("四川", (2, 3));
    shanghai.insert("重庆", (2, 2));
    shanghai.insert("贵州", (3, 3));
    shanghai.insert("云南", (3, 4));
    matrix.insert("上海仓", shanghai);

    // 珠海仓 (owned) — faster for southern provinces
    let mut zhuhai = HashMap::new();
    zhuhai.insert("上海", (2, 3));
    zhuhai.insert("江苏", (2, 3));
    zhuhai.insert("浙江", (2, 2));
    zhuhai.insert("安徽", (2, 3));
    zhuhai.insert("北京", (3, 3));
    zhuhai.insert("天津", (3, 3));
    zhuhai.insert("河北", (3, 3));
    zhuhai.insert("山东", (3, 3));
    zhuhai.insert("河南", (2, 3));
    zhuhai.insert("山西", (3, 4));
    zhuhai.insert("湖北", (2, 2));
    zhuhai.insert("湖南", (2, 2));
    zhuhai.insert("江西", (2, 2));
    zhuhai.insert("福建", (2, 2));
    zhuhai.insert("广东", (1, 1));
    zhuhai.insert("广西", (1, 1));
    zhuhai.insert("海南", (1, 2));
    zhuhai.insert("辽宁", (3, 4));
    zhuhai.insert("吉林", (4, 4));
    zhuhai.insert("黑龙江", (4, 5));
    zhuhai.insert("内蒙古", (4, 5));
    zhuhai.insert("陕西", (3, 4));
    zhuhai.insert("甘肃", (4, 5));
    zhuhai.insert("宁夏", (4, 5));
    zhuhai.insert("青海", (4, 5));
    zhuhai.insert("新疆", (5, 6));
    zhuhai.insert("西藏", (5, 6));
    zhuhai.insert("四川", (2, 3));
    zhuhai.insert("重庆", (2, 3));
    zhuhai.insert("贵州", (2, 2));
    zhuhai.insert("云南", (2, 3));
    matrix.insert("珠海仓", zhuhai);

    // 南昌友仓 (partner) — only serves 湖南, 湖北, 江西
    let mut nanchang = HashMap::new();
    nanchang.insert("湖南", (1, 1));
    nanchang.insert("湖北", (1, 1));
    nanchang.insert("江西", (1, 1));
    matrix.insert("南昌友仓", nanchang);

    matrix
}

#[allow(clippy::type_complexity)]
static LAZY_LOGISTICS_MATRIX: std::sync::LazyLock<
    HashMap<&'static str, HashMap<&'static str, (i64, i64)>>,
> = std::sync::LazyLock::new(build_logistics_matrix);

/// All 31 provinces served by 上海仓.
const ALL_PROVINCES: &[&str] = &[
    "上海",
    "江苏",
    "浙江",
    "安徽",
    "北京",
    "天津",
    "河北",
    "山东",
    "河南",
    "山西",
    "湖北",
    "湖南",
    "江西",
    "福建",
    "广东",
    "广西",
    "海南",
    "辽宁",
    "吉林",
    "黑龙江",
    "内蒙古",
    "陕西",
    "甘肃",
    "宁夏",
    "青海",
    "新疆",
    "西藏",
    "四川",
    "重庆",
    "贵州",
    "云南",
];

// ── Public API ───────────────────────────────────────────────────

/// Query logistics estimate for a province to a specific warehouse.
/// Returns { shippingDays, returnDays } or None if not serviced.
pub fn query_sf_express(province: &str, warehouse_name: &str) -> Option<LogisticsEstimate> {
    let matrix = LAZY_LOGISTICS_MATRIX.get(warehouse_name)?;
    let (shipping_days, return_days) = matrix.get(province)?;
    Some(LogisticsEstimate {
        shipping_days: *shipping_days,
        return_days: *return_days,
    })
}

/// Estimate shipping cost and days from warehouse to province.
/// Alias for query_sf_express. Returns default estimates if not found.
pub fn estimate_shipping(province: &str, warehouse_name: &str) -> Option<LogisticsEstimate> {
    query_sf_express(province, warehouse_name).or_else(default_estimate)
}

/// Estimate return cost and days from province back to warehouse.
/// Alias for query_sf_express. Returns default estimates if not found.
pub fn estimate_return(province: &str, warehouse_name: &str) -> Option<LogisticsEstimate> {
    query_sf_express(province, warehouse_name).or_else(default_estimate)
}

/// Default fallback estimates.
fn default_estimate() -> Option<LogisticsEstimate> {
    Some(LogisticsEstimate {
        shipping_days: 2,
        return_days: 2,
    })
}

/// Get all 31 provinces.
pub fn get_all_provinces() -> Vec<&'static str> {
    ALL_PROVINCES.to_vec()
}

/// Get logistics matrix for all warehouses for a given province.
pub fn get_province_logistics(province: &str) -> HashMap<&'static str, LogisticsEstimate> {
    let mut result = HashMap::new();
    for (wh_name, matrix) in LAZY_LOGISTICS_MATRIX.iter() {
        if let Some(&(shipping_days, return_days)) = matrix.get(province) {
            result.insert(
                *wh_name,
                LogisticsEstimate {
                    shipping_days,
                    return_days,
                },
            );
        }
    }
    result
}
