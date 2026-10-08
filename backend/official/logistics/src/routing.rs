//! 仓库路线解析模块 (leaf — 无跨模块依赖)
//!
//! 命令:
//! - resolve_warehouse: 根据省份确定最佳发货 + 退货仓库
//! - get_occupancy_coefficients: 获取占用系数
//! - list_regions: 列出所有省份

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use system_core::*;

// ── 输入类型 ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolveWarehouseInput {
    pub province: String,
}

impl Validate for ResolveWarehouseInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.province.trim().is_empty() {
            errors.push(FieldError {
                field: "province".into(),
                message: "省份不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for ResolveWarehouseInput {
    fn sanitize(&mut self) {
        self.province = self.province.trim().into();
    }
}

// ── 领域类型 ──

#[derive(Debug, Clone)]
struct RegionRule {
    pub warehouse_id: String,
    pub warehouse_name: String,
    pub warehouse_type: String,
    #[allow(dead_code)]
    pub enabled: bool,
    #[allow(dead_code)]
    pub province: String,
    pub shipping_days: i64,
    pub return_days: i64,
}

// ── 输出类型 ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WarehouseRoute {
    pub send_warehouse_id: String,
    pub send_warehouse_name: String,
    pub return_warehouse_id: String,
    pub return_warehouse_name: String,
    pub shipping_days: i64,
    pub return_days: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OccupancyCoefficients {
    pub shipping: f64,
    pub normal: f64,
    #[serde(rename = "return")]
    pub r#return: f64,
}

// ── 常量 ──

const OCCUPANCY_COEFFICIENT_SHIPPING: f64 = 0.2;
const OCCUPANCY_COEFFICIENT_NORMAL: f64 = 1.0;
const OCCUPANCY_COEFFICIENT_RETURN: f64 = 0.2;

// ── 模块主体 ──

pub struct FeatureWarehouseRouting;

impl FeatureWarehouseRouting {
    pub fn new() -> Self {
        Self
    }

    fn do_get_occupancy_coefficients(&self) -> OccupancyCoefficients {
        OccupancyCoefficients {
            shipping: OCCUPANCY_COEFFICIENT_SHIPPING,
            normal: OCCUPANCY_COEFFICIENT_NORMAL,
            r#return: OCCUPANCY_COEFFICIENT_RETURN,
        }
    }

    fn do_resolve_warehouse(
        &self,
        _province: &str,
        rules: &[RegionRule],
    ) -> Option<WarehouseRoute> {
        if rules.is_empty() {
            return None;
        }

        let owned_rules: Vec<&RegionRule> = rules
            .iter()
            .filter(|r| r.warehouse_type == "owned")
            .collect();

        let best_send = &rules[0];
        let shipping_days = best_send.shipping_days;

        let mut best_return: Option<&RegionRule> = None;
        let mut return_days = i64::MAX;

        for rule in rules {
            let is_this_owned = rule.warehouse_type == "owned";
            if !is_this_owned && rule.warehouse_id != best_send.warehouse_id {
                continue;
            }
            if rule.return_days < return_days {
                return_days = rule.return_days;
                best_return = Some(rule);
            }
        }

        if best_return.is_none() && best_send.warehouse_type == "owned" {
            let other_owned = owned_rules
                .iter()
                .find(|r| r.warehouse_id != best_send.warehouse_id);
            if let Some(other) = other_owned {
                best_return = Some(other);
                return_days = other.return_days;
            }
        }

        let best_return = best_return.unwrap_or(best_send);
        if return_days == i64::MAX {
            return_days = best_send.return_days;
        }

        Some(WarehouseRoute {
            send_warehouse_id: best_send.warehouse_id.clone(),
            send_warehouse_name: best_send.warehouse_name.clone(),
            return_warehouse_id: best_return.warehouse_id.clone(),
            return_warehouse_name: best_return.warehouse_name.clone(),
            shipping_days,
            return_days,
        })
    }
}

impl Default for FeatureWarehouseRouting {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemModule for FeatureWarehouseRouting {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "warehouse_routing".into(),
            version: "0.1.0".into(),
            description: "仓库路线解析 — 根据省份确定发货/退货仓库".into(),
            author: "hoshi".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "resolve_warehouse",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "get_occupancy_coefficients",
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
            "resolve_warehouse" => {
                DeserializeGuard::default().check_raw(&payload)?;

                let unvalidated: Unvalidated<ResolveWarehouseInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();

                // 此模块需要从 talos-backend 传入 rules。
                // 独立运行时返回空。
                let rules: Vec<RegionRule> = Vec::new();
                let result = self.do_resolve_warehouse(&input.province, &rules);

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
            "get_occupancy_coefficients" => {
                let result = self.do_get_occupancy_coefficients();
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
            name: "warehouse_routing".into(),
            description: "仓库路线解析".into(),
            commands: vec![
                CommandSchema {
                    name: "resolve_warehouse".into(),
                    description: "根据省份确定最佳发货+退货仓库".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "get_occupancy_coefficients".into(),
                    description: "获取占用系数".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
            ],
        }
    }
}
