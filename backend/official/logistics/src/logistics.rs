//! 快递物流矩阵模块 (leaf — 无跨模块依赖)
//!
//! SF Express 31省 × 3仓库的时效估算矩阵（硬编码）。
//! 命令: get_coefficient, estimate_shipping, estimate_return, get_all_provinces

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use system_core::*;

// ── 输入类型 ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EstimateInput {
    pub province: String,
    pub warehouse_name: String,
}

impl Validate for EstimateInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.province.trim().is_empty() {
            errors.push(FieldError {
                field: "province".into(),
                message: "省份不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if self.warehouse_name.trim().is_empty() {
            errors.push(FieldError {
                field: "warehouseName".into(),
                message: "仓库名称不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for EstimateInput {
    fn sanitize(&mut self) {
        self.province = self.province.trim().into();
        self.warehouse_name = self.warehouse_name.trim().into();
    }
}

// ── 领域/输出类型 ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogisticsEstimate {
    pub shipping_days: i64,
    pub return_days: i64,
}

// ── 静态物流矩阵 ──
// (与 Node.js logistics-service.js 中相同的硬编码数据)

#[allow(clippy::type_complexity)]
static LAZY_LOGISTICS_MATRIX: std::sync::LazyLock<
    HashMap<&'static str, HashMap<&'static str, (i64, i64)>>,
> = std::sync::LazyLock::new(|| {
    let mut matrix = HashMap::new();

    let mut shanghai: HashMap<&str, (i64, i64)> = HashMap::new();
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

    let mut zhuhai: HashMap<&str, (i64, i64)> = HashMap::new();
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

    let mut nanchang: HashMap<&str, (i64, i64)> = HashMap::new();
    nanchang.insert("湖南", (1, 1));
    nanchang.insert("湖北", (1, 1));
    nanchang.insert("江西", (1, 1));
    matrix.insert("南昌友仓", nanchang);

    matrix
});

// ── 模块主体 ──

pub struct FeatureLogistics;

impl FeatureLogistics {
    pub fn new() -> Self {
        Self
    }

    fn do_query_sf_express(
        &self,
        province: &str,
        warehouse_name: &str,
    ) -> Option<LogisticsEstimate> {
        let matrix = LAZY_LOGISTICS_MATRIX.get(warehouse_name)?;
        let (shipping_days, return_days) = matrix.get(province)?;
        Some(LogisticsEstimate {
            shipping_days: *shipping_days,
            return_days: *return_days,
        })
    }

    fn do_get_all_provinces(&self) -> Vec<&'static str> {
        Self::ALL_PROVINCES.to_vec()
    }

    const ALL_PROVINCES: &'static [&'static str] = &[
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
}

impl Default for FeatureLogistics {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemModule for FeatureLogistics {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "logistics".into(),
            version: "0.1.0".into(),
            description: "快递物流矩阵 — SF Express 31省×3仓库时效估算".into(),
            author: "hoshi".into(),
            wasm_compatible: false,
            storage: None,
        }
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "get_coefficient",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "estimate_shipping",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "estimate_return",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "get_all_provinces",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
        ]
    }

    fn init(&mut self, _config: Value) -> Result<(), String> {
        Ok(())
    }

    fn execute(
        &self,
        command: &str,
        payload: Value,
        _ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        match command {
            "estimate_shipping" | "estimate_return" | "get_coefficient" => {
                DeserializeGuard::default().check_raw(&payload)?;

                let unvalidated: Unvalidated<EstimateInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();

                let result = self
                    .do_query_sf_express(&input.province, &input.warehouse_name)
                    .unwrap_or(LogisticsEstimate {
                        shipping_days: 2,
                        return_days: 2,
                    });

                serde_json::to_value(result).map_err(|e| {
                    serde_json::to_string(&ErrorPayload {
                        category: "sys".into(),
                        code: "SYS_SERIALIZE".into(),
                        message: e.to_string(),
                        field: None,
                        context: None,
                    })
                    .unwrap_or_default()
                })
            }
            "get_all_provinces" => {
                let result = self.do_get_all_provinces();
                serde_json::to_value(result).map_err(|e| {
                    serde_json::to_string(&ErrorPayload {
                        category: "sys".into(),
                        code: "SYS_SERIALIZE".into(),
                        message: e.to_string(),
                        field: None,
                        context: None,
                    })
                    .unwrap_or_default()
                })
            }
            _ => Err(serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_UNKNOWN_COMMAND".into(),
                message: format!("未知命令: {}", command),
                field: None,
                context: None,
            })
            .unwrap_or_default()),
        }
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "logistics".into(),
            description: "快递物流矩阵".into(),
            commands: vec![
                CommandSchema {
                    name: "get_coefficient".into(),
                    description: "查询省份→仓库的快递时效".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "estimate_shipping".into(),
                    description: "估算发货时效".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "estimate_return".into(),
                    description: "估算退货时效".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "get_all_provinces".into(),
                    description: "获取所有31个省份列表".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
            ],
        }
    }
}
