use super::machine_api::{Scope, machine_scope_allowed};

#[test]
fn machine_scopes_cannot_become_interactive_identity_or_compliance_authority() {
    for module in ["two_fa", "consent", "deletion", "user_settings"] {
        assert!(
            !machine_scope_allowed(&Scope {
                module: module.to_string(),
                command: "any".to_string(),
            }),
            "interactive module {module} must remain unavailable to machine principals"
        );
    }

    assert!(machine_scope_allowed(&Scope {
        module: "probe".to_string(),
        command: "read".to_string(),
    }));
}
