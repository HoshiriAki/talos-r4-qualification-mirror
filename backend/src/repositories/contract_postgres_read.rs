#![cfg(feature = "postgres")]

use serde_json::Value;
use sqlx::{Postgres, QueryBuilder, Row};

use crate::repositories::RepositoryError;
use crate::repositories::contract::{
    ContractListItem, ContractListProjection, ContractProjection, ContractSignatureProjection,
    ContractTemplateListProjection, ContractTemplateProjection,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn template_list(
    session: &RepositorySession,
    is_active: Option<bool>,
    page: i64,
    page_size: i64,
) -> Result<ContractTemplateListProjection, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let page = page.max(1);
    let page_size = page_size.clamp(1, 100);

    session.pg_read(move |connection| {
        Box::pin(async move {
            let offset = (page - 1) * page_size;
            let mut count = QueryBuilder::<Postgres>::new(
                "SELECT COUNT(*)::bigint FROM contract_templates WHERE tenant_id=",
            );
            count.push_bind(&tenant_id);
            if let Some(active) = is_active {
                count
                    .push(" AND is_active=")
                    .push_bind(if active { 1_i32 } else { 0_i32 });
            }
            let total: i64 = count
                .build_query_scalar()
                .fetch_one(&mut *connection)
                .await?;

            let mut data = QueryBuilder::<Postgres>::new(
                "SELECT id,name,content_json,(is_active <> 0) AS is_active,created_at,updated_at
                 FROM contract_templates WHERE tenant_id=",
            );
            data.push_bind(&tenant_id);
            if let Some(active) = is_active {
                data.push(" AND is_active=")
                    .push_bind(if active { 1_i32 } else { 0_i32 });
            }
            data.push(" ORDER BY created_at DESC LIMIT ")
                .push_bind(page_size)
                .push(" OFFSET ")
                .push_bind(offset);
            let rows = data.build().fetch_all(&mut *connection).await?;
            let items = rows
                .iter()
                .map(map_template)
                .collect::<Result<Vec<_>, _>>()?;

            Ok(ContractTemplateListProjection {
                items,
                page,
                page_size,
                total,
            })
        })
    })
}

pub(in crate::repositories) fn template_get(
    session: &RepositorySession,
    id: i64,
) -> Result<Option<ContractTemplateProjection>, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    session.pg_read(move |connection| {
        Box::pin(async move {
            sqlx::query(
                "SELECT id,name,content_json,(is_active <> 0) AS is_active,created_at,updated_at
                 FROM contract_templates
                 WHERE tenant_id=$1 AND id=$2 LIMIT 1",
            )
            .bind(&tenant_id)
            .bind(id)
            .fetch_optional(&mut *connection)
            .await?
            .map(|row| map_template(&row))
            .transpose()
        })
    })
}

pub(in crate::repositories) fn list(
    session: &RepositorySession,
    order_id: Option<&str>,
    customer_phone: Option<&str>,
    status: Option<&str>,
    page: i64,
    page_size: i64,
) -> Result<ContractListProjection, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let order_id = order_id
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let customer_phone = customer_phone
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let status = status.filter(|value| !value.is_empty()).map(str::to_owned);
    let page = page.max(1);
    let page_size = page_size.clamp(1, 100);

    session.pg_read(move |connection| {
        Box::pin(async move {
            let offset = (page - 1) * page_size;

            let mut count =
                QueryBuilder::<Postgres>::new("SELECT COUNT(*)::bigint FROM contracts WHERE ");
            push_contract_filters(
                &mut count,
                &tenant_id,
                order_id.as_deref(),
                customer_phone.as_deref(),
                status.as_deref(),
            );
            let total: i64 = count
                .build_query_scalar()
                .fetch_one(&mut *connection)
                .await?;

            let mut data = QueryBuilder::<Postgres>::new(
                "SELECT id,order_id,template_id,customer_name,customer_phone,
                        device_value::double precision AS device_value,
                        content_json,status,signed_at,created_at,updated_at
                 FROM contracts WHERE ",
            );
            push_contract_filters(
                &mut data,
                &tenant_id,
                order_id.as_deref(),
                customer_phone.as_deref(),
                status.as_deref(),
            );
            data.push(" ORDER BY created_at DESC LIMIT ")
                .push_bind(page_size)
                .push(" OFFSET ")
                .push_bind(offset);

            let rows = data.build().fetch_all(&mut *connection).await?;
            let items = rows
                .iter()
                .map(map_contract_list_item)
                .collect::<Result<Vec<_>, _>>()?;

            Ok(ContractListProjection {
                items,
                page,
                page_size,
                total,
            })
        })
    })
}

pub(in crate::repositories) fn get(
    session: &RepositorySession,
    id: i64,
) -> Result<Option<ContractProjection>, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    session.pg_read(move |connection| {
        Box::pin(async move {
            let base = sqlx::query(
                "SELECT id,order_id,template_id,customer_name,customer_phone,
                        device_value::double precision AS device_value,
                        content_json,status,signed_at,created_at,updated_at
                 FROM contracts
                 WHERE tenant_id=$1 AND id=$2 LIMIT 1",
            )
            .bind(&tenant_id)
            .bind(id)
            .fetch_optional(&mut *connection)
            .await?;

            let Some(base) = base else {
                return Ok(None);
            };

            let signature_rows = sqlx::query(
                "SELECT id,signer_name,signer_phone,signature_data,signed_at
                 FROM e_signatures
                 WHERE tenant_id=$1 AND contract_id=$2
                 ORDER BY signed_at ASC,id ASC",
            )
            .bind(&tenant_id)
            .bind(id)
            .fetch_all(&mut *connection)
            .await?;
            let signatures = signature_rows
                .iter()
                .map(map_signature)
                .collect::<Result<Vec<_>, _>>()?;

            Ok(Some(ContractProjection {
                id: base.try_get("id")?,
                order_id: base.try_get("order_id")?,
                template_id: base.try_get("template_id")?,
                customer_name: base.try_get("customer_name")?,
                customer_phone: base.try_get("customer_phone")?,
                device_value: base.try_get("device_value")?,
                content_json: parse_json(base.try_get::<String, _>("content_json")?),
                status: base.try_get("status")?,
                signed_at: base.try_get("signed_at")?,
                created_at: base.try_get("created_at")?,
                updated_at: base.try_get("updated_at")?,
                signatures,
            }))
        })
    })
}

fn push_contract_filters<'args>(
    query: &mut QueryBuilder<'args, Postgres>,
    tenant_id: &'args str,
    order_id: Option<&'args str>,
    customer_phone: Option<&'args str>,
    status: Option<&'args str>,
) {
    query.push("tenant_id=").push_bind(tenant_id);
    if let Some(order_id) = order_id {
        query.push(" AND order_id=").push_bind(order_id);
    }
    if let Some(customer_phone) = customer_phone {
        query.push(" AND customer_phone=").push_bind(customer_phone);
    }
    if let Some(status) = status {
        query.push(" AND status=").push_bind(status);
    }
}

fn map_template(row: &sqlx::postgres::PgRow) -> Result<ContractTemplateProjection, sqlx::Error> {
    Ok(ContractTemplateProjection {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        content_json: parse_json(row.try_get::<String, _>("content_json")?),
        is_active: row.try_get("is_active")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn map_contract_list_item(row: &sqlx::postgres::PgRow) -> Result<ContractListItem, sqlx::Error> {
    Ok(ContractListItem {
        id: row.try_get("id")?,
        order_id: row.try_get("order_id")?,
        template_id: row.try_get("template_id")?,
        customer_name: row.try_get("customer_name")?,
        customer_phone: row.try_get("customer_phone")?,
        device_value: row.try_get("device_value")?,
        content_json: parse_json(row.try_get::<String, _>("content_json")?),
        status: row.try_get("status")?,
        signed_at: row.try_get("signed_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn map_signature(row: &sqlx::postgres::PgRow) -> Result<ContractSignatureProjection, sqlx::Error> {
    Ok(ContractSignatureProjection {
        id: row.try_get("id")?,
        signer_name: row.try_get("signer_name")?,
        signer_phone: row.try_get("signer_phone")?,
        signature_data: row.try_get("signature_data")?,
        signed_at: row.try_get("signed_at")?,
    })
}

fn parse_json(value: String) -> Value {
    serde_json::from_str(&value).unwrap_or_default()
}
