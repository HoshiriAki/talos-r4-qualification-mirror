#![cfg(feature = "experimental-orchestration")]

use serde_json::json;
use system_core::{
    ErrorPayload,
    experimental::{CompensationLog, ModuleOp, OrchestrationError, SagaStep},
};

fn accepts_preserved_contract(_: ModuleOp, _: SagaStep, _: OrchestrationError, _: CompensationLog) {
}

#[test]
fn feature_exposes_preserved_data_contract_without_runtime_behavior() {
    let forward = ModuleOp {
        module_name: "orders".into(),
        command: "create".into(),
        payload: json!({ "order": "o-1" }),
        timeout_ms: Some(5_000),
    };
    assert_eq!(forward.module_name, "orders");
    assert_eq!(forward.command, "create");
    assert_eq!(forward.payload, json!({ "order": "o-1" }));
    assert_eq!(forward.timeout_ms, Some(5_000));

    let step = SagaStep {
        forward: ModuleOp {
            module_name: "orders".into(),
            command: "create".into(),
            payload: json!(null),
            timeout_ms: None,
        },
        compensate: None,
    };
    assert_eq!(step.forward.module_name, "orders");
    assert!(step.compensate.is_none());

    let log = CompensationLog {
        step: 1,
        module: "orders".into(),
        command: "cancel".into(),
        result: Ok(json!({ "cancelled": true })),
    };
    assert_eq!(log.step, 1);
    assert!(log.result.is_ok());

    let failure = OrchestrationError {
        step: 2,
        module: "payments".into(),
        command: "charge".into(),
        error: ErrorPayload {
            category: "biz".into(),
            code: "BIZ_DECLINED".into(),
            message: "declined".into(),
            field: None,
            context: None,
        },
        compensated: Vec::new(),
    };
    assert_eq!(failure.step, 2);
    assert_eq!(failure.error.code, "BIZ_DECLINED");
    assert!(failure.compensated.is_empty());

    accepts_preserved_contract(
        forward,
        step,
        failure,
        CompensationLog {
            step: 0,
            module: String::new(),
            command: String::new(),
            result: Err(String::new()),
        },
    );
}
