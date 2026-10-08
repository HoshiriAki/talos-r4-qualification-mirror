use std::sync::Arc;

use official_order::FeatureOrder;
use serde::Deserialize;
use serde_json::Value;
use system_core::{
    AccessRequirement, CommandMetadata, CommandSchema, EffectClass, ErrorPayload, ExecutionContext,
    ModuleMetadata, ModuleSchema, SimulationSupport, SystemModule,
};

use crate::domain::AllocationId;
use crate::repositories::{
    DeviceAllocationRequest, DraftOrderPatch, ImportedOrderDraft, RepositoryProvider,
};

use super::{OrderLifecycleV2Module, OrderReadCompatibilityModule};

/// One-version compatibility wrapper for the legacy `order.transition` command.
///
/// Read/export compatibility remains backed by the official Order projection. All business writes
/// exposed by this module are named application/repository or Lifecycle V2 commands; the legacy
/// generic create/update/delete commands are retired at this boundary.
pub struct OrderLifecycleCompatibilityModule {
    lifecycle: OrderLifecycleV2Module,
    reads: OrderReadCompatibilityModule,
    repositories: Arc<dyn RepositoryProvider>,
}

impl OrderLifecycleCompatibilityModule {
    pub fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            lifecycle: OrderLifecycleV2Module::new(repository_provider.clone()),
            reads: OrderReadCompatibilityModule::new(repository_provider.clone()),
            repositories: repository_provider,
        }
    }

    fn lifecycle_view(&self, order_id: &str, ctx: &ExecutionContext) -> Result<Value, String> {
        self.lifecycle.execute(
            "get_lifecycle",
            serde_json::json!({ "orderId": order_id }),
            ctx,
        )
    }

    fn version(view: &Value) -> Result<i64, String> {
        view.get("lifecycle")
            .and_then(|value| value.get("version"))
            .and_then(Value::as_i64)
            .ok_or_else(|| "canonical lifecycle version missing".to_string())
    }

    fn dimension_status<'a>(view: &'a Value, key: &str) -> Option<&'a str> {
        view.get("lifecycle")
            .and_then(|value| value.get(key))
            .and_then(Value::as_str)
    }

    fn apply(
        &self,
        order_id: &str,
        action: &str,
        expected_version: i64,
        reason: &str,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        self.lifecycle.execute(
            action,
            serde_json::json!({
                "orderId": order_id,
                "expectedVersion": expected_version,
                "reason": reason,
            }),
            ctx,
        )
    }

    fn execute_legacy_transition(
        &self,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let order_id = payload
            .get("orderId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "legacy transition requires orderId".to_string())?;
        let target = payload
            .get("toStatus")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "legacy transition requires toStatus".to_string())?;
        let reason = payload
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();

        let mut view = self.lifecycle_view(order_id, ctx)?;

        // The legacy paid edge collapsed billing/payment into one state. Preserve the old public
        // target while executing the two canonical financial facts explicitly.
        if target == "paid" && Self::dimension_status(&view, "financialStatus") == Some("unbilled")
        {
            view = self.apply(
                order_id,
                "mark_awaiting_payment",
                Self::version(&view)?,
                reason,
                ctx,
            )?;
        }

        // The old in_use -> returned edge skipped the return-pending operational fact.
        if target == "returned"
            && Self::dimension_status(&view, "fulfilmentStatus") == Some("in_use")
        {
            view = self.apply(
                order_id,
                "mark_return_pending",
                Self::version(&view)?,
                reason,
                ctx,
            )?;
        }

        let action = match target {
            "confirmed" => "confirm_order",
            "paid" => "record_paid",
            "shipped" => "mark_shipped",
            "in_use" => "mark_in_use",
            "returned" => "mark_returned",
            "inspected" => "mark_inspected",
            "repairing" => "mark_repairing",
            "completed" => "mark_completed",
            "closed" => "close_order",
            "cancelled" => "cancel_order",
            _ => {
                return Err(format!(
                    "legacy transition target has no Lifecycle V2 named-action mapping: {target}"
                ));
            }
        };

        let result = self.apply(order_id, action, Self::version(&view)?, reason, ctx)?;
        Ok(serde_json::json!({
            "ok": true,
            "to": target,
            "lifecycle": result,
        }))
    }

    fn named_draft_update(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let order_id = payload
            .get("orderId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("{command} requires orderId"))?;
        let lifecycle = self.lifecycle_view(order_id, ctx)?;
        let expected_version = payload
            .get("expectedVersion")
            .and_then(Value::as_i64)
            .filter(|version| *version > 0)
            .ok_or_else(|| format!("{command} requires a positive expectedVersion"))?;
        let actual_version = Self::version(&lifecycle)?;
        if expected_version != actual_version {
            return Err(format!(
                "stale Order version: expected {expected_version}, actual {actual_version}"
            ));
        }
        if Self::dimension_status(&lifecycle, "commercialStatus") != Some("draft") {
            return Err(format!(
                "{command} is only allowed while the Order is draft"
            ));
        }

        let allowed: &[&str] = match command {
            "change_draft_dates" => &["startDate", "endDate", "deliveryDate"],
            "change_draft_address" => &["address", "province", "pickupMethods"],
            "change_notes" => &["notes"],
            _ => return Err(format!("unknown named draft update: {command}")),
        };
        let mut allowed_fields = vec!["orderId", "expectedVersion"];
        allowed_fields.extend_from_slice(allowed);
        reject_unknown_fields(&payload, command, &allowed_fields)?;
        let patch = DraftOrderPatch {
            start_date: optional_string(&payload, "startDate")?,
            end_date: optional_string(&payload, "endDate")?,
            delivery_date: optional_string(&payload, "deliveryDate")?,
            pickup_methods: optional_string_vec(&payload, "pickupMethods")?,
            address: optional_string(&payload, "address")?,
            province: optional_string(&payload, "province")?,
            notes: optional_string(&payload, "notes")?,
        };
        if !patch.has_changes() {
            return Err(format!("{command} requires at least one named field"));
        }
        let scoped = self
            .repositories
            .bind(ctx)
            .map_err(|error| error.to_string())?;
        scoped
            .order_commands()
            .update_draft(order_id, expected_version, patch)
            .map_err(|error| error.to_string())?;

        // Preserve the established compatibility response envelope while keeping the write
        // itself inside the scoped repository command above.
        self.reads
            .execute("get_order", serde_json::json!({ "id": order_id }), ctx)
    }

    fn named_lifecycle_action(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let order_id = payload
            .get("orderId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("{command} requires orderId"))?;
        let reason = payload.get("reason").and_then(Value::as_str).unwrap_or("");
        let expected_version = payload
            .get("expectedVersion")
            .and_then(Value::as_i64)
            .filter(|version| *version > 0)
            .ok_or_else(|| format!("{command} requires a positive expectedVersion"))?;
        reject_unknown_fields(&payload, command, &["orderId", "expectedVersion", "reason"])?;
        self.apply(order_id, command, expected_version, reason, ctx)
    }

    fn import_order(&self, payload: Value, ctx: &ExecutionContext) -> Result<Value, String> {
        let input: ImportOrderInput = serde_json::from_value(payload)
            .map_err(|error| format!("invalid import Order payload: {error}"))?;
        input.validate()?;
        let scoped = self
            .repositories
            .bind(ctx)
            .map_err(|error| error.to_string())?;
        let order = scoped
            .order_commands()
            .create_imported_draft(input.into_draft())
            .map_err(|error| error.to_string())?;
        Ok(serde_json::json!({ "ok": true, "order": order }))
    }

    fn allocate_device_compat(
        &self,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let order_id = required_string(&payload, "orderId")?;
        let serial_no = required_string(&payload, "deviceSerialNo")?;
        let scoped = self
            .repositories
            .bind(ctx)
            .map_err(|error| error.to_string())?;
        let reservation = scoped
            .reservations()
            .find_by_order(order_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "canonical reservation not found for Order".to_string())?;
        let allocation = scoped
            .reservations()
            .allocate_device(&reservation.id, AllocationId::new(), serial_no)
            .map_err(|error| error.to_string())?;
        serde_json::to_value(allocation).map_err(|error| error.to_string())
    }

    fn allocate_devices_batch(
        &self,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let rows = payload
            .get("rows")
            .and_then(Value::as_array)
            .ok_or_else(|| "allocate_devices_batch requires a rows array".to_string())?;
        if rows.is_empty() {
            return Err("allocate_devices_batch requires at least one row".into());
        }
        let mut requests = Vec::with_capacity(rows.len());
        for row in rows {
            reject_unknown_fields(
                row,
                "allocate_devices_batch",
                &["rowNo", "orderId", "deviceSerialNo"],
            )?;
            requests.push(DeviceAllocationRequest {
                order_id: required_string(row, "orderId")?.to_owned(),
                device_serial_no: required_string(row, "deviceSerialNo")?.to_owned(),
            });
        }
        let scoped = self
            .repositories
            .bind(ctx)
            .map_err(|error| error.to_string())?;
        let allocations = scoped
            .reservations()
            .allocate_devices_batch(&requests)
            .map_err(|error| error.to_string())?;
        Ok(serde_json::json!({
            "ok": true,
            "allocationCount": allocations.len(),
            "allocations": allocations,
        }))
    }

    fn validate_allocate_device(
        &self,
        payload: &Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let order_id = required_string(payload, "orderId")?;
        let serial_no = required_string(payload, "deviceSerialNo")?;
        let scoped = self
            .repositories
            .bind(ctx)
            .map_err(|error| error.to_string())?;
        let reservation = scoped
            .reservations()
            .find_by_order(order_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "canonical reservation not found for Order".to_string())?;
        if reservation.status != "confirmed" {
            return Err("device allocation requires confirmed reservation".into());
        }
        if reservation.allocations.iter().any(|allocation| {
            allocation.status == "allocated" && allocation.device_serial_no == serial_no
        }) {
            return Err("device already has an active canonical Allocation".into());
        }
        Ok(serde_json::json!({
            "ok": true,
            "orderId": order_id,
            "reservationId": reservation.id.as_str(),
            "deviceSerialNo": serial_no,
        }))
    }

    fn release_device_compat(
        &self,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let order_id = required_string(&payload, "orderId")?;
        let serial_no = required_string(&payload, "deviceSerialNo")?;
        let lifecycle = self.lifecycle_view(order_id, ctx)?;
        let fulfilment = Self::dimension_status(&lifecycle, "fulfilmentStatus").unwrap_or("");
        if !matches!(fulfilment, "unplanned" | "reserved" | "allocated") {
            return Err(format!(
                "device release is unsafe while fulfilmentStatus is {fulfilment}"
            ));
        }
        let scoped = self
            .repositories
            .bind(ctx)
            .map_err(|error| error.to_string())?;
        let reservation = scoped
            .reservations()
            .find_by_order(order_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "canonical reservation not found for Order".to_string())?;
        let allocation = reservation
            .allocations
            .into_iter()
            .find(|allocation| {
                allocation.status == "allocated" && allocation.device_serial_no == serial_no
            })
            .ok_or_else(|| "active canonical allocation not found".to_string())?;
        let released = scoped
            .reservations()
            .release_allocation(&allocation.id)
            .map_err(|error| error.to_string())?;
        serde_json::to_value(released).map_err(|error| error.to_string())
    }

    fn validate_fixture_dispatch(
        &self,
        payload: &Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let order_id = required_string(payload, "orderId")?;
        let scoped = self
            .repositories
            .bind(ctx)
            .map_err(|error| error.to_string())?;
        let reservation = scoped
            .reservations()
            .find_by_order(order_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "canonical reservation not found for Order".to_string())?;
        if reservation.status != "confirmed" {
            return Err("fixture dispatch requires a confirmed reservation".into());
        }
        let allocated: Vec<_> = reservation
            .allocations
            .iter()
            .filter(|allocation| allocation.status == "allocated")
            .map(|allocation| allocation.device_serial_no.clone())
            .collect();
        if allocated.is_empty() {
            return Err("fixture dispatch requires canonical Allocation facts".into());
        }
        let requested: Vec<String> = payload
            .get("deviceSerialNos")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        if !requested.is_empty() && requested.iter().any(|serial| !allocated.contains(serial)) {
            return Err("requested device list does not match canonical Allocations".into());
        }
        let lifecycle = self.lifecycle_view(order_id, ctx)?;
        let can_dispatch = lifecycle
            .get("allowedActions")
            .and_then(Value::as_array)
            .is_some_and(|actions| {
                actions.iter().any(|action| {
                    action.get("action").and_then(Value::as_str) == Some("mark_ready_to_ship")
                })
            });
        if !can_dispatch {
            return Err("canonical lifecycle does not allow fixture dispatch".into());
        }
        Ok(serde_json::json!({
            "ok": true,
            "orderId": order_id,
            "reservationId": reservation.id.as_str(),
            "allocatedDevices": allocated,
        }))
    }

    fn fixture_dispatch(&self, payload: Value, ctx: &ExecutionContext) -> Result<Value, String> {
        let validated = self.validate_fixture_dispatch(&payload, ctx)?;
        let order_id = required_string(&payload, "orderId")?;
        let reason = "deterministic fixture dispatch";
        let ready_view = self.lifecycle_view(order_id, ctx)?;
        let ready = self.apply(
            order_id,
            "mark_ready_to_ship",
            Self::version(&ready_view)?,
            reason,
            ctx,
        )?;
        let shipped = self.apply(
            order_id,
            "mark_shipped",
            Self::version(&ready)?,
            reason,
            ctx,
        )?;
        Ok(serde_json::json!({
            "ok": true,
            "orderId": order_id,
            "intentKind": "fixture_dispatch",
            "trackingNo": payload.get("trackingNo").cloned().unwrap_or(Value::String(String::new())),
            "validation": validated,
            "lifecycle": shipped,
        }))
    }

    fn export_excel(&self, payload: Value, ctx: &ExecutionContext) -> Result<Value, String> {
        let page = self.reads.execute("list_orders", payload, ctx)?;
        let orders = page
            .get("users")
            .and_then(Value::as_array)
            .ok_or_else(|| "order read compatibility returned no users array".to_string())?;

        use rust_xlsxwriter::{Format, Workbook};

        let mut workbook = Workbook::new();
        let header_fmt = Format::new().set_bold();
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("订单列表").map_err(xlsx_error)?;

        let headers = [
            "订单号",
            "开始日期",
            "结束日期",
            "发货日期",
            "状态",
            "省份",
            "设备数量",
            "总金额",
            "收货地址",
            "备注",
            "创建时间",
        ];
        for (index, header) in headers.iter().enumerate() {
            worksheet
                .write_string_with_format(0, index as u16, *header, &header_fmt)
                .ok();
        }

        for (index, order) in orders.iter().enumerate() {
            let row = (index + 1) as u32;
            worksheet
                .write_string(row, 0, json_string(order, "orderNo"))
                .ok();
            worksheet
                .write_string(row, 1, json_string(order, "startDate"))
                .ok();
            worksheet
                .write_string(row, 2, json_string(order, "endDate"))
                .ok();
            worksheet
                .write_string(row, 3, json_string(order, "deliveryDate"))
                .ok();
            worksheet
                .write_string(row, 4, json_string(order, "status"))
                .ok();
            worksheet
                .write_string(row, 5, json_string(order, "province"))
                .ok();
            let device_count = order
                .get("devices")
                .and_then(Value::as_array)
                .map_or(0.0, |devices| devices.len() as f64);
            worksheet.write_number(row, 6, device_count).ok();
            worksheet
                .write_number(
                    row,
                    7,
                    order
                        .get("totalPrice")
                        .and_then(Value::as_f64)
                        .unwrap_or_default(),
                )
                .ok();
            worksheet
                .write_string(row, 8, json_string(order, "address"))
                .ok();
            worksheet
                .write_string(row, 9, json_string(order, "notes"))
                .ok();
            worksheet
                .write_string(row, 10, json_string(order, "createdAt"))
                .ok();
        }

        for column in 0..11u16 {
            worksheet.set_column_width(column, 16).ok();
        }

        let buffer = workbook.save_to_buffer().map_err(xlsx_error)?;
        let timestamp: String = crate::utils::time::shanghai_now_iso()
            .chars()
            .filter(|character| character.is_ascii_digit())
            .take(14)
            .collect();

        Ok(serde_json::json!({
            "fileName": format!("订单导出-{timestamp}.xlsx"),
            "content": base64_encode(&buffer),
            "mimeType": "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        }))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ImportOrderInput {
    order_no: String,
    start_date: String,
    end_date: String,
    delivery_date: String,
    pickup_methods: Vec<String>,
    #[serde(default)]
    address: String,
    #[serde(default)]
    notes: String,
}

impl ImportOrderInput {
    fn validate(&self) -> Result<(), String> {
        if self.order_no.is_empty()
            || !self
                .order_no
                .chars()
                .all(|character| character.is_ascii_digit())
        {
            return Err("import orderNo must contain ASCII digits only".into());
        }
        for (field, value) in [
            ("startDate", self.start_date.as_str()),
            ("endDate", self.end_date.as_str()),
            ("deliveryDate", self.delivery_date.as_str()),
        ] {
            if value.len() != 10
                || value.as_bytes()[4] != b'-'
                || value.as_bytes()[7] != b'-'
                || !value.chars().enumerate().all(|(index, character)| {
                    index == 4 || index == 7 || character.is_ascii_digit()
                })
            {
                return Err(format!("{field} must use YYYY-MM-DD"));
            }
        }
        if self.start_date > self.end_date {
            return Err("startDate must not be after endDate".into());
        }
        if self.pickup_methods.is_empty() {
            return Err("pickupMethods must not be empty".into());
        }
        Ok(())
    }

    fn into_draft(self) -> ImportedOrderDraft {
        ImportedOrderDraft {
            reconciliation_order_no: self.order_no,
            start_date: self.start_date,
            end_date: self.end_date,
            delivery_date: self.delivery_date,
            pickup_methods: self.pickup_methods,
            address: self.address,
            notes: self.notes,
        }
    }
}

fn required_string<'a>(payload: &'a Value, field: &str) -> Result<&'a str, String> {
    payload
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{field} must not be blank"))
}

fn reject_unknown_fields(payload: &Value, command: &str, allowed: &[&str]) -> Result<(), String> {
    let object = payload
        .as_object()
        .ok_or_else(|| format!("{command} payload must be an object"))?;
    if let Some(field) = object
        .keys()
        .find(|field| !allowed.contains(&field.as_str()))
    {
        return Err(format!("{command} does not accept field {field}"));
    }
    Ok(())
}

fn optional_string(payload: &Value, field: &str) -> Result<Option<String>, String> {
    match payload.get(field) {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.trim().to_string())),
        Some(_) => Err(format!("{field} must be a string")),
    }
}

fn optional_string_vec(payload: &Value, field: &str) -> Result<Option<Vec<String>>, String> {
    match payload.get(field) {
        None => Ok(None),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(|item| item.trim().to_string())
                    .ok_or_else(|| format!("{field} must contain only strings"))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        Some(_) => Err(format!("{field} must be an array of strings")),
    }
}

impl SystemModule for OrderLifecycleCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureOrder::new().metadata()
    }

    fn init(&mut self, _config: Value) -> Result<(), String> {
        Ok(())
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        const REPLACED_COMMANDS: &[&str] = &[
            "create_order",
            "update_order",
            "delete_order",
            "change_draft_dates",
            "change_draft_address",
            "change_notes",
            "submit_order",
            "cancel_order",
            "import_order",
            "allocate_device",
            "allocate_devices_batch",
            "release_device",
            "validate_fixture_dispatch",
            "validate_allocate_device",
            "fixture_dispatch",
        ];
        let mut commands: Vec<_> = FeatureOrder::new()
            .commands()
            .into_iter()
            .filter(|metadata| !REPLACED_COMMANDS.contains(&metadata.name))
            .collect();
        commands.extend([
            named_write_command("change_draft_dates"),
            named_write_command("change_draft_address"),
            named_write_command("change_notes"),
            named_write_command("submit_order"),
            named_write_command("cancel_order"),
            named_write_command("import_order"),
            named_write_command("allocate_device"),
            named_write_command("allocate_devices_batch"),
            named_write_command("release_device"),
            read_command("validate_fixture_dispatch"),
            read_command("validate_allocate_device"),
            named_write_command("fixture_dispatch"),
        ]);
        commands
    }

    fn execute(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        if command == "transition" {
            return self.execute_legacy_transition(payload, ctx);
        }
        match command {
            "change_draft_dates" | "change_draft_address" | "change_notes" => {
                self.named_draft_update(command, payload, ctx)
            }
            "submit_order" | "cancel_order" => self.named_lifecycle_action(command, payload, ctx),
            "import_order" => self.import_order(payload, ctx),
            "allocate_device" => self.allocate_device_compat(payload, ctx),
            "allocate_devices_batch" => self.allocate_devices_batch(payload, ctx),
            "validate_allocate_device" => self.validate_allocate_device(&payload, ctx),
            "release_device" => self.release_device_compat(payload, ctx),
            "validate_fixture_dispatch" => self.validate_fixture_dispatch(&payload, ctx),
            "fixture_dispatch" => self.fixture_dispatch(payload, ctx),
            "create_order" | "update_order" | "delete_order" => Err(format!(
                "legacy generic command {command} is retired; use named Order commands"
            )),
            "list_orders" | "get_order" => self.reads.execute(command, payload, ctx),
            "export_excel" => self.export_excel(payload, ctx),
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

    fn schema(&self) -> ModuleSchema {
        const REPLACED_COMMANDS: &[&str] = &[
            "create_order",
            "update_order",
            "delete_order",
            "change_draft_dates",
            "change_draft_address",
            "change_notes",
            "submit_order",
            "cancel_order",
            "import_order",
            "allocate_device",
            "allocate_devices_batch",
            "release_device",
            "validate_fixture_dispatch",
            "validate_allocate_device",
            "fixture_dispatch",
        ];
        let mut schema = FeatureOrder::new().schema();
        schema
            .commands
            .retain(|command| !REPLACED_COMMANDS.contains(&command.name.as_str()));
        schema.commands.extend(
            [
                "change_draft_dates",
                "change_draft_address",
                "change_notes",
                "submit_order",
                "cancel_order",
                "import_order",
                "allocate_device",
                "allocate_devices_batch",
                "release_device",
                "validate_fixture_dispatch",
                "validate_allocate_device",
                "fixture_dispatch",
            ]
            .into_iter()
            .map(|name| CommandSchema {
                name: name.into(),
                description: "R1 named Order compatibility command".into(),
                version: "1.0.0".into(),
                input_schema: None,
                output_schema: None,
            }),
        );
        schema
    }
}

fn named_write_command(name: &'static str) -> CommandMetadata {
    CommandMetadata::new(
        name,
        AccessRequirement::Authenticated,
        &[EffectClass::DatabaseWrite],
        SimulationSupport::Blocked,
    )
}

fn read_command(name: &'static str) -> CommandMetadata {
    CommandMetadata::new(
        name,
        AccessRequirement::Authenticated,
        &[EffectClass::DatabaseRead],
        SimulationSupport::Blocked,
    )
}

fn json_string<'a>(value: &'a Value, field: &str) -> &'a str {
    value.get(field).and_then(Value::as_str).unwrap_or("")
}

fn xlsx_error(error: rust_xlsxwriter::XlsxError) -> String {
    serde_json::to_string(&ErrorPayload {
        category: "sys".into(),
        code: "SYS_XLSX".into(),
        message: error.to_string(),
        field: None,
        context: None,
    })
    .unwrap_or_default()
}

fn base64_encode(bytes: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3f) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3f) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3f) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{OrderLifecycleCompatibilityModule, named_write_command};
    use system_core::{EffectClass, SimulationSupport};

    #[test]
    fn compatibility_wrapper_is_an_explicit_one_version_boundary() {
        let type_name = std::any::type_name::<OrderLifecycleCompatibilityModule>();
        assert!(type_name.contains("OrderLifecycleCompatibilityModule"));
    }

    #[test]
    fn named_order_updates_are_database_writes_and_simulation_blocked() {
        for command in [
            "change_draft_dates",
            "change_draft_address",
            "change_notes",
            "submit_order",
            "cancel_order",
            "import_order",
            "allocate_device",
            "release_device",
            "fixture_dispatch",
        ] {
            let metadata = named_write_command(command);
            assert_eq!(metadata.name, command);
            assert!(metadata.effects.contains(&EffectClass::DatabaseWrite));
            assert_eq!(metadata.simulation, SimulationSupport::Blocked);
        }
    }
}
