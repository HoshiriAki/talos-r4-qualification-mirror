use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::post;
use axum::{Json, Router};
use official_order::report::{FeatureReport, RevenueData};
use serde::Deserialize;
use std::sync::Arc;

use crate::error::AppError;
use crate::middleware::tenant_extractors::TrustedTenantUser;
use crate::state::AppState;
use crate::utils::http as http_utils;

pub fn report_routes() -> Router<Arc<AppState>> {
    Router::new().route("/api/reports/revenue", post(revenue_report))
}

#[derive(Debug, Deserialize)]
struct RevenueBody {
    #[serde(default)]
    start_date: String,
    #[serde(default)]
    end_date: String,
}

async fn revenue_report(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
    Json(body): Json<RevenueBody>,
) -> Result<Response, AppError> {
    let data = state
        .registry
        .execute(
            "report",
            "get_revenue_data",
            serde_json::json!({
                "startDate": body.start_date,
                "endDate": body.end_date,
            }),
            tenant_user.context(),
        )
        .map_err(AppError::from_error_payload)?;
    let data: RevenueData = serde_json::from_value(data)?;
    let buffer = FeatureReport::build_excel_bytes(&data).map_err(AppError::from_error_payload)?;
    let file_name = format!(
        "revenue-report-{}.xlsx",
        crate::utils::time::format_export_timestamp()
    );

    Ok(http_utils::binary_response(
        StatusCode::OK,
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        &file_name,
        buffer,
    ))
}
