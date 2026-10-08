use rusqlite::{OptionalExtension, params};
use serde::Serialize;

use crate::repositories::session::RepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoaBasisProjection {
    pub device_serial_no: String,
    pub purchase_price: f64,
    pub total_revenue: f64,
    pub first_order_date: Option<String>,
}

pub(in crate::repositories) struct SqliteRoaRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> SqliteRoaRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub(in crate::repositories) fn get(
        &self,
        device_serial_no: &str,
    ) -> Result<Option<RoaBasisProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let serial = device_serial_no.to_owned();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT
                        ap.device_serial_no,
                        ap.purchase_price,
                        COALESCE((
                            SELECT SUM(opd.amount)
                            FROM order_price_details opd
                            JOIN orders o
                              ON o.id=opd.orderId
                            JOIN order_devices od
                              ON od.orderId=opd.orderId
                            WHERE o.tenant_id=ap.tenant_id
                              AND od.tenant_id=ap.tenant_id
                              AND od.serialNo=ap.device_serial_no
                              AND o.status IN ('completed','returned','inspected','in_use','paid','shipped')
                        ),0),
                        (
                            SELECT MIN(opd.dateKey)
                            FROM order_price_details opd
                            JOIN orders o
                              ON o.id=opd.orderId
                            JOIN order_devices od
                              ON od.orderId=opd.orderId
                            WHERE o.tenant_id=ap.tenant_id
                              AND od.tenant_id=ap.tenant_id
                              AND od.serialNo=ap.device_serial_no
                        )
                     FROM asset_purchases ap
                     WHERE ap.tenant_id=?1 AND ap.device_serial_no=?2
                     LIMIT 1",
                    params![tenant_id, serial],
                    map_roa_basis,
                )
                .optional()
        })
    }

    pub(in crate::repositories) fn list(&self) -> Result<Vec<RoaBasisProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT
                    ap.device_serial_no,
                    ap.purchase_price,
                    COALESCE((
                        SELECT SUM(opd.amount)
                        FROM order_price_details opd
                        JOIN orders o
                          ON o.id=opd.orderId
                        JOIN order_devices od
                          ON od.orderId=opd.orderId
                        WHERE o.tenant_id=ap.tenant_id
                          AND od.tenant_id=ap.tenant_id
                          AND od.serialNo=ap.device_serial_no
                          AND o.status IN ('completed','returned','inspected','in_use','paid','shipped')
                    ),0),
                    (
                        SELECT MIN(opd.dateKey)
                        FROM order_price_details opd
                        JOIN orders o
                          ON o.id=opd.orderId
                        JOIN order_devices od
                          ON od.orderId=opd.orderId
                        WHERE o.tenant_id=ap.tenant_id
                          AND od.tenant_id=ap.tenant_id
                          AND od.serialNo=ap.device_serial_no
                    )
                 FROM asset_purchases ap
                 WHERE ap.tenant_id=?1
                 ORDER BY ap.device_serial_no",
            )?;
            statement
                .query_map([tenant_id], map_roa_basis)?
                .collect::<Result<Vec<_>, _>>()
        })
    }

    fn tenant_id(&self) -> String {
        self.session.binding().tenant_id().as_str().to_owned()
    }
}

fn map_roa_basis(row: &rusqlite::Row<'_>) -> rusqlite::Result<RoaBasisProjection> {
    Ok(RoaBasisProjection {
        device_serial_no: row.get(0)?,
        purchase_price: row.get(1)?,
        total_revenue: row.get(2)?,
        first_order_date: row.get(3)?,
    })
}
