use chrono::{SecondsFormat, Utc};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::repositories::sqlite::SqliteRepositorySession;
use crate::repositories::workflow::append_outbox_tx;
use crate::repositories::{RepositoryError, ScopedRepositories};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnProjection {
    pub id: String,
    pub order_id: String,
    pub status: String,
    pub version: i64,
    pub item_count: i64,
    pub required_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectionProjection {
    pub id: String,
    pub allocation_id: String,
    pub device_serial_no: String,
    pub status: String,
    pub version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettlementProjection {
    pub id: String,
    pub order_id: String,
    pub currency: String,
    pub amount_minor: i64,
    pub facts_hash: String,
    pub status: String,
    pub blocker_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RentalClosureFacts {
    pub return_received: bool,
    pub inspection_complete: bool,
    pub open_damage_reviews: i64,
    pub settlement_terminal: bool,
}

pub struct ScopedRentalClosureRepository<'a> {
    session: &'a SqliteRepositorySession,
}

impl<'a> ScopedRentalClosureRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub fn receive_allocation(
        &self,
        order_id: &str,
        allocation_id: &str,
        receive_identity: &str,
    ) -> Result<ReturnProjection, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let order_id = required(order_id, "orderId")?.to_owned();
        let allocation_id = required(allocation_id, "allocationId")?.to_owned();
        let receive_identity = required(receive_identity, "receiveIdentity")?.to_owned();
        let now = utc_now();
        self.session.write_immediate(|tx| {
            let allocation: Option<(String, String)> = tx.query_row(
                "SELECT a.id,a.device_serial_no FROM allocations a JOIN rental_reservations r ON r.tenant_id=a.tenant_id AND r.id=a.reservation_id WHERE a.tenant_id=?1 AND r.order_id=?2 AND a.id=?3 AND a.status='allocated'",
                params![tenant, order_id, allocation_id], |row| Ok((row.get(0)?, row.get(1)?)),
            ).optional().map_err(sqlite)?;
            let (_, device_serial_no) = allocation.ok_or_else(|| RepositoryError::ContractViolation("allocation does not belong to the active tenant/order".into()))?;
            let return_id: String = tx.query_row(
                "SELECT id FROM rental_returns WHERE tenant_id=?1 AND order_id=?2",
                params![tenant, order_id], |row| row.get(0),
            ).optional().map_err(sqlite)?.unwrap_or_else(|| Uuid::new_v4().to_string());
            tx.execute(
                "INSERT OR IGNORE INTO rental_returns (id,tenant_id,order_id,status,version,created_at,updated_at) VALUES (?1,?2,?3,'receiving',1,?4,?4)",
                params![return_id, tenant, order_id, now],
            ).map_err(sqlite)?;

            let existing_identity: Option<String> = tx.query_row(
                "SELECT allocation_id FROM rental_return_items WHERE tenant_id=?1 AND receive_identity=?2",
                params![tenant, receive_identity], |row| row.get(0),
            ).optional().map_err(sqlite)?;
            if let Some(existing) = existing_identity {
                if existing != allocation_id {
                    return Err(RepositoryError::ContractViolation("receiveIdentity is already bound to another allocation".into()));
                }
            } else {
                let item_id = Uuid::new_v4().to_string();
                tx.execute(
                    "INSERT OR IGNORE INTO rental_return_items (id,tenant_id,return_id,allocation_id,device_serial_no,receive_identity,status,received_at) VALUES (?1,?2,?3,?4,?5,?6,'received',?7)",
                    params![item_id, tenant, return_id, allocation_id, device_serial_no, receive_identity, now],
                ).map_err(sqlite)?;
                tx.execute(
                    "INSERT OR IGNORE INTO rental_inspections (id,tenant_id,return_item_id,allocation_id,device_serial_no,status,version,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,'pending',1,?6,?6)",
                    params![Uuid::new_v4().to_string(), tenant, item_id, allocation_id, device_serial_no, now],
                ).map_err(sqlite)?;
            }

            let required_count: i64 = tx.query_row(
                "SELECT COUNT(*) FROM allocations a JOIN rental_reservations r ON r.tenant_id=a.tenant_id AND r.id=a.reservation_id WHERE a.tenant_id=?1 AND r.order_id=?2 AND a.status='allocated'",
                params![tenant, order_id], |row| row.get(0),
            ).map_err(sqlite)?;
            let item_count: i64 = tx.query_row(
                "SELECT COUNT(*) FROM rental_return_items WHERE tenant_id=?1 AND return_id=?2",
                params![tenant, return_id], |row| row.get(0),
            ).map_err(sqlite)?;
            if required_count > 0 && item_count == required_count {
                let changed = tx.execute("UPDATE rental_returns SET status='received',received_at=?3,updated_at=?3,version=version+1 WHERE tenant_id=?1 AND id=?2 AND status='receiving'", params![tenant,return_id,now]).map_err(sqlite)?;
                if changed == 1 {
                    append_outbox_tx(tx,&tenant,"return",&return_id,"ReturnReceived",&format!("return-received:{return_id}"),&serde_json::json!({"orderId":order_id,"returnId":return_id}),&now)?;
                }
            }
            load_return(tx, &tenant, &return_id)
        })
    }

    pub fn transition_inspection(
        &self,
        inspection_id: &str,
        target: &str,
        expected_version: i64,
    ) -> Result<InspectionProjection, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let inspection_id = required(inspection_id, "inspectionId")?.to_owned();
        let target = required(target, "target")?.to_owned();
        let now = utc_now();
        self.session.write_immediate(|tx| {
            let (current, order_id): (String,String) = tx.query_row(
                "SELECT i.status,r.order_id FROM rental_inspections i JOIN rental_return_items ri ON ri.tenant_id=i.tenant_id AND ri.id=i.return_item_id JOIN rental_returns r ON r.tenant_id=ri.tenant_id AND r.id=ri.return_id WHERE i.tenant_id=?1 AND i.id=?2 AND i.version=?3",
                params![tenant,inspection_id,expected_version], |row| Ok((row.get(0)?,row.get(1)?)),
            ).map_err(|_| RepositoryError::ContractViolation("inspection missing or optimistic version conflict".into()))?;
            let legal = matches!((current.as_str(),target.as_str()),("pending","in_progress") | ("in_progress","passed") | ("in_progress","failed"));
            if !legal { return Err(RepositoryError::ContractViolation(format!("illegal inspection transition: {current} -> {target}"))); }
            tx.execute("UPDATE rental_inspections SET status=?1,version=version+1,started_at=CASE WHEN ?1='in_progress' THEN ?4 ELSE started_at END,completed_at=CASE WHEN ?1 IN ('passed','failed') THEN ?4 ELSE completed_at END,updated_at=?4 WHERE tenant_id=?2 AND id=?3 AND version=?5", params![target,tenant,inspection_id,now,expected_version]).map_err(sqlite)?;
            if target == "failed" {
                tx.execute("INSERT OR IGNORE INTO rental_damage_reviews (id,tenant_id,inspection_id,order_id,status,created_at) VALUES (?1,?2,?3,?4,'open',?5)",params![Uuid::new_v4().to_string(),tenant,inspection_id,order_id,now]).map_err(sqlite)?;
            }
            emit_inspection_complete_if_ready(tx,&tenant,&order_id,&now)?;
            load_inspection(tx, &tenant, &inspection_id).map_err(sqlite)
        })
    }

    pub fn resolve_damage_review(
        &self,
        inspection_id: &str,
        note: &str,
    ) -> Result<(), RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let inspection_id = required(inspection_id, "inspectionId")?.to_owned();
        let note = required(note, "resolutionNote")?.to_owned();
        let now = utc_now();
        self.session.write_immediate(|tx| {
            let order_id: String = tx.query_row("SELECT order_id FROM rental_damage_reviews WHERE tenant_id=?1 AND inspection_id=?2 AND status='open'",params![tenant,inspection_id],|row| row.get(0)).map_err(|_| RepositoryError::ContractViolation("open damage review not found".into()))?;
            tx.execute("UPDATE rental_damage_reviews SET status='reviewed',resolution_note=?3,resolved_at=?4 WHERE tenant_id=?1 AND inspection_id=?2 AND status='open'",params![tenant,inspection_id,note,now]).map_err(sqlite)?;
            let open: i64 = tx.query_row("SELECT COUNT(*) FROM rental_damage_reviews WHERE tenant_id=?1 AND order_id=?2 AND status='open'",params![tenant,order_id],|row| row.get(0)).map_err(sqlite)?;
            if open == 0 { append_outbox_tx(tx,&tenant,"order",&order_id,"RiskCasesResolved",&format!("risk-cases-resolved:{order_id}"),&serde_json::json!({"orderId":order_id}),&now)?; }
            Ok(())
        })
    }

    pub fn calculate_settlement(
        &self,
        order_id: &str,
        currency: &str,
        amount_minor: i64,
        financial_authority_available: bool,
    ) -> Result<SettlementProjection, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let order_id = required(order_id, "orderId")?.to_owned();
        let currency = required(currency, "currency")?.to_ascii_uppercase();
        if currency.len() != 3 {
            return Err(RepositoryError::ContractViolation(
                "currency must be ISO three-letter semantics".into(),
            ));
        }
        let now = utc_now();
        self.session.write_immediate(|tx| {
            let terminal: i64 = tx.query_row("SELECT COUNT(*) FROM rental_inspections i JOIN rental_return_items ri ON ri.tenant_id=i.tenant_id AND ri.id=i.return_item_id JOIN rental_returns r ON r.tenant_id=ri.tenant_id AND r.id=ri.return_id WHERE i.tenant_id=?1 AND r.order_id=?2 AND i.status IN ('passed','failed')",params![tenant,order_id],|row| row.get(0)).map_err(sqlite)?;
            let open: i64=tx.query_row("SELECT COUNT(*) FROM rental_damage_reviews WHERE tenant_id=?1 AND order_id=?2 AND status='open'",params![tenant,order_id],|row| row.get(0)).map_err(sqlite)?;
            let facts=format!("order={order_id};terminal={terminal};open={open};currency={currency};amount_minor={amount_minor}");
            let facts_hash=format!("sha256:{:x}",Sha256::digest(facts.as_bytes()));
            let (status,blocker)=if financial_authority_available && open==0 { ("calculated",None) } else { ("blocked",Some(if open>0 {"DAMAGE_REVIEW_OPEN"} else {"FINANCIAL_AUTHORITY_NOT_AVAILABLE"})) };
            tx.execute("INSERT INTO rental_settlements (id,tenant_id,order_id,currency,amount_minor,facts_hash,status,blocker_code,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?9) ON CONFLICT(tenant_id,order_id) DO UPDATE SET currency=excluded.currency,amount_minor=excluded.amount_minor,facts_hash=excluded.facts_hash,status=excluded.status,blocker_code=excluded.blocker_code,updated_at=excluded.updated_at",params![Uuid::new_v4().to_string(),tenant,order_id,currency,amount_minor,facts_hash,status,blocker,now]).map_err(sqlite)?;
            load_settlement(tx,&tenant,&order_id)
        })
    }

    #[cfg(test)]
    pub fn mark_fixture_settlement_terminal(&self, order_id: &str) -> Result<(), RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let order_id = required(order_id, "orderId")?.to_owned();
        let now = utc_now();
        self.session.write_immediate(|tx| { let changed=tx.execute("UPDATE rental_settlements SET status='terminal',blocker_code=NULL,updated_at=?3 WHERE tenant_id=?1 AND order_id=?2 AND status='calculated'",params![tenant,order_id,now]).map_err(sqlite)?; if changed!=1{return Err(RepositoryError::ContractViolation("settlement is not calculated by an explicit deterministic fixture".into()));} append_outbox_tx(tx,&tenant,"order",&order_id,"SettlementComplete",&format!("settlement-complete:{order_id}"),&serde_json::json!({"orderId":order_id}),&now)?; Ok(()) })
    }

    pub fn inspection(&self, id: &str) -> Result<Option<InspectionProjection>, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let id = id.to_owned();
        self.session
            .read(|c| load_inspection(c, &tenant, &id).optional())
            .map_err(|e| e)
    }

    pub fn facts(&self, order_id: &str) -> Result<RentalClosureFacts, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let order_id = required(order_id, "orderId")?.to_owned();
        self.session.read(|connection| {
            let return_received: i64 = connection.query_row(
                "SELECT COUNT(*) FROM rental_returns WHERE tenant_id=?1 AND order_id=?2 AND status='received'",
                params![tenant, order_id], |row| row.get(0),
            )?;
            let counts: (i64,i64) = connection.query_row(
                "SELECT COUNT(*),COALESCE(SUM(CASE WHEN i.status IN ('passed','failed') THEN 1 ELSE 0 END),0) FROM rental_inspections i JOIN rental_return_items ri ON ri.tenant_id=i.tenant_id AND ri.id=i.return_item_id JOIN rental_returns r ON r.tenant_id=ri.tenant_id AND r.id=ri.return_id WHERE i.tenant_id=?1 AND r.order_id=?2",
                params![tenant, order_id], |row| Ok((row.get(0)?,row.get(1)?)),
            )?;
            let open_damage_reviews: i64 = connection.query_row("SELECT COUNT(*) FROM rental_damage_reviews WHERE tenant_id=?1 AND order_id=?2 AND status='open'",params![tenant,order_id],|row|row.get(0))?;
            let settlement_terminal: i64 = connection.query_row("SELECT COUNT(*) FROM rental_settlements WHERE tenant_id=?1 AND order_id=?2 AND status='terminal'",params![tenant,order_id],|row|row.get(0))?;
            Ok(RentalClosureFacts{return_received:return_received==1,inspection_complete:counts.0>0&&counts.0==counts.1,open_damage_reviews,settlement_terminal:settlement_terminal==1})
        })
    }
}

fn emit_inspection_complete_if_ready(
    tx: &rusqlite::Transaction<'_>,
    tenant: &str,
    order_id: &str,
    now: &str,
) -> Result<(), RepositoryError> {
    let counts:(i64,i64)=tx.query_row("SELECT COUNT(*),SUM(CASE WHEN i.status IN ('passed','failed') THEN 1 ELSE 0 END) FROM rental_inspections i JOIN rental_return_items ri ON ri.tenant_id=i.tenant_id AND ri.id=i.return_item_id JOIN rental_returns r ON r.tenant_id=ri.tenant_id AND r.id=ri.return_id WHERE i.tenant_id=?1 AND r.order_id=?2",params![tenant,order_id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(sqlite)?;
    if counts.0 > 0 && counts.0 == counts.1 {
        append_outbox_tx(
            tx,
            tenant,
            "order",
            order_id,
            "InspectionComplete",
            &format!("inspection-complete:{order_id}"),
            &serde_json::json!({"orderId":order_id}),
            now,
        )?;
        let open:i64=tx.query_row("SELECT COUNT(*) FROM rental_damage_reviews WHERE tenant_id=?1 AND order_id=?2 AND status='open'",params![tenant,order_id],|r|r.get(0)).map_err(sqlite)?;
        if open == 0 {
            append_outbox_tx(
                tx,
                tenant,
                "order",
                order_id,
                "RiskCasesResolved",
                &format!("risk-cases-resolved:{order_id}"),
                &serde_json::json!({"orderId":order_id}),
                now,
            )?;
        }
    }
    Ok(())
}
fn load_return(
    c: &rusqlite::Connection,
    t: &str,
    id: &str,
) -> Result<ReturnProjection, RepositoryError> {
    c.query_row("SELECT r.id,r.order_id,r.status,r.version,(SELECT COUNT(*) FROM rental_return_items ri WHERE ri.tenant_id=r.tenant_id AND ri.return_id=r.id),(SELECT COUNT(*) FROM allocations a JOIN rental_reservations rr ON rr.tenant_id=a.tenant_id AND rr.id=a.reservation_id WHERE a.tenant_id=r.tenant_id AND rr.order_id=r.order_id AND a.status='allocated') FROM rental_returns r WHERE r.tenant_id=?1 AND r.id=?2",params![t,id],|r|Ok(ReturnProjection{id:r.get(0)?,order_id:r.get(1)?,status:r.get(2)?,version:r.get(3)?,item_count:r.get(4)?,required_count:r.get(5)?})).map_err(sqlite)
}
fn load_inspection(
    c: &rusqlite::Connection,
    t: &str,
    id: &str,
) -> Result<InspectionProjection, rusqlite::Error> {
    c.query_row("SELECT id,allocation_id,device_serial_no,status,version FROM rental_inspections WHERE tenant_id=?1 AND id=?2",params![t,id],|r|Ok(InspectionProjection{id:r.get(0)?,allocation_id:r.get(1)?,device_serial_no:r.get(2)?,status:r.get(3)?,version:r.get(4)?}))
}
fn load_settlement(
    c: &rusqlite::Connection,
    t: &str,
    o: &str,
) -> Result<SettlementProjection, RepositoryError> {
    c.query_row("SELECT id,order_id,currency,amount_minor,facts_hash,status,blocker_code FROM rental_settlements WHERE tenant_id=?1 AND order_id=?2",params![t,o],|r|Ok(SettlementProjection{id:r.get(0)?,order_id:r.get(1)?,currency:r.get(2)?,amount_minor:r.get(3)?,facts_hash:r.get(4)?,status:r.get(5)?,blocker_code:r.get(6)?})).map_err(sqlite)
}
fn required<'a>(v: &'a str, n: &str) -> Result<&'a str, RepositoryError> {
    let v = v.trim();
    if v.is_empty() {
        Err(RepositoryError::ContractViolation(format!(
            "{n} must not be blank"
        )))
    } else {
        Ok(v)
    }
}
fn utc_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn sqlite(e: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(e.to_string())
}
