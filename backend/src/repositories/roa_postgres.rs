#![cfg(feature = "postgres")]

use sqlx::Row;

use crate::repositories::RepositoryError;
use crate::repositories::roa::RoaBasisProjection;
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresRoaRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresRoaRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn get(
        &self,
        device_serial_no: &str,
    ) -> Result<Option<RoaBasisProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let serial = device_serial_no.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT
                        ap.device_serial_no,
                        ap.purchase_price::double precision AS purchase_price,
                        COALESCE((
                            SELECT SUM(opd.amount)::double precision
                            FROM order_price_details opd
                            JOIN orders o
                              ON o.id=opd.orderid
                            JOIN order_devices od
                              ON od.orderid=opd.orderid
                            WHERE o.tenant_id=ap.tenant_id
                              AND od.tenant_id=ap.tenant_id
                              AND od.serialno=ap.device_serial_no
                              AND o.status IN ('completed','returned','inspected','in_use','paid','shipped')
                        ),0::double precision) AS total_revenue,
                        (
                            SELECT MIN(opd.datekey)
                            FROM order_price_details opd
                            JOIN orders o
                              ON o.id=opd.orderid
                            JOIN order_devices od
                              ON od.orderid=opd.orderid
                            WHERE o.tenant_id=ap.tenant_id
                              AND od.tenant_id=ap.tenant_id
                              AND od.serialno=ap.device_serial_no
                        ) AS first_order_date
                     FROM asset_purchases ap
                     WHERE ap.tenant_id=$1 AND ap.device_serial_no=$2
                     LIMIT 1",
                )
                .bind(tenant_id)
                .bind(serial)
                .fetch_optional(&mut *connection)
                .await?;
                row.as_ref().map(map_roa_basis).transpose()
            })
        })
    }

    pub(in crate::repositories) fn list(&self) -> Result<Vec<RoaBasisProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "SELECT
                        ap.device_serial_no,
                        ap.purchase_price::double precision AS purchase_price,
                        COALESCE((
                            SELECT SUM(opd.amount)::double precision
                            FROM order_price_details opd
                            JOIN orders o
                              ON o.id=opd.orderid
                            JOIN order_devices od
                              ON od.orderid=opd.orderid
                            WHERE o.tenant_id=ap.tenant_id
                              AND od.tenant_id=ap.tenant_id
                              AND od.serialno=ap.device_serial_no
                              AND o.status IN ('completed','returned','inspected','in_use','paid','shipped')
                        ),0::double precision) AS total_revenue,
                        (
                            SELECT MIN(opd.datekey)
                            FROM order_price_details opd
                            JOIN orders o
                              ON o.id=opd.orderid
                            JOIN order_devices od
                              ON od.orderid=opd.orderid
                            WHERE o.tenant_id=ap.tenant_id
                              AND od.tenant_id=ap.tenant_id
                              AND od.serialno=ap.device_serial_no
                        ) AS first_order_date
                     FROM asset_purchases ap
                     WHERE ap.tenant_id=$1
                     ORDER BY ap.device_serial_no",
                )
                .bind(tenant_id)
                .fetch_all(&mut *connection)
                .await?
                .iter()
                .map(map_roa_basis)
                .collect()
            })
        })
    }

    fn tenant_id(&self) -> String {
        self.session.binding().tenant_id().as_str().to_owned()
    }
}

fn map_roa_basis(row: &sqlx::postgres::PgRow) -> Result<RoaBasisProjection, sqlx::Error> {
    Ok(RoaBasisProjection {
        device_serial_no: row.try_get("device_serial_no")?,
        purchase_price: row.try_get("purchase_price")?,
        total_revenue: row.try_get("total_revenue")?,
        first_order_date: row.try_get("first_order_date")?,
    })
}
