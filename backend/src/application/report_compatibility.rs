use official_order::report::{ExportExcelInput, FeatureReport, GetRevenueDataInput};
use serde_json::Value;
use system_core::{
    AccessRequirement, CommandMetadata, DeserializeGuard, EffectClass, ErrorPayload,
    ExecutionContext, ModuleMetadata, ModuleSchema, SimulationSupport, SystemModule, Unvalidated,
};

use crate::repositories::{ReportCompatibilityRepository, RepositoryError};

#[derive(Clone)]
pub(crate) struct ReportCompatibilityModule {
    repository: ReportCompatibilityRepository,
}

impl ReportCompatibilityModule {
    pub(crate) fn new(repository: ReportCompatibilityRepository) -> Self {
        Self { repository }
    }

    fn repository_error(error: RepositoryError) -> String {
        serde_json::to_string(&ErrorPayload {
            category: "sys".into(),
            code: error.code().into(),
            message: "report persistence unavailable".into(),
            field: None,
            context: None,
        })
        .unwrap_or_else(|_| "SYS_REPORT_PERSISTENCE".into())
    }
}

impl SystemModule for ReportCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "report".into(),
            version: "0.1.0".into(),
            description: "营收报表模块 — 按省份/月度聚合查询 + Excel 导出".into(),
            author: "hoshi".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn init(&mut self, _config: Value) -> Result<(), String> {
        Ok(())
    }

    fn execute(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        match command {
            "get_revenue_data" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<GetRevenueDataInput> = payload.try_into()?;
                let input = unvalidated.sanitize().validate()?.into_inner();
                let data = self
                    .repository
                    .revenue_data(
                        ctx.data_scope().tenant_id().as_str(),
                        &input.start_date,
                        &input.end_date,
                    )
                    .map_err(Self::repository_error)?;
                serde_json::to_value(data).map_err(|error| error.to_string())
            }
            "export_excel" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<ExportExcelInput> = payload.try_into()?;
                let input = unvalidated.sanitize().validate()?.into_inner();
                let data = self
                    .repository
                    .revenue_data(
                        ctx.data_scope().tenant_id().as_str(),
                        &input.start_date,
                        &input.end_date,
                    )
                    .map_err(Self::repository_error)?;
                let output = FeatureReport::build_excel(&data, &input.label)?;
                serde_json::to_value(output).map_err(|error| error.to_string())
            }
            _ => Err(serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_UNKNOWN_COMMAND".into(),
                message: format!("未知命令: {command}"),
                field: None,
                context: None,
            })
            .unwrap_or_default()),
        }
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "get_revenue_data",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "export_excel",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
        ]
    }

    fn schema(&self) -> ModuleSchema {
        FeatureReport::new().schema()
    }
}
