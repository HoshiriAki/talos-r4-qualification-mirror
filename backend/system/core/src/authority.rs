use serde::{Deserialize, Serialize};

macro_rules! domain_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, String> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(concat!(stringify!($name), " must not be blank").into());
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

domain_id!(IdentityId);
domain_id!(TenantMembershipId);
domain_id!(PlatformMembershipId);
domain_id!(AuthSessionId);
domain_id!(PreviewSessionId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantRole {
    Staff,
    Admin,
    Owner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformRole {
    #[serde(rename = "platform_owner")]
    Owner,
    #[serde(rename = "platform_admin")]
    Admin,
    #[serde(rename = "platform_operator")]
    Operator,
    SupportEngineer,
    BusinessOperator,
    SecurityAuditor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformCapability {
    PlatformOverviewRead,
    PlatformHealthRead,
    PlatformOperationsManage,
    TenantList,
    TenantRead,
    TenantCreate,
    TenantUpdate,
    TenantSuspend,
    TenantDelete,
    TenantGovernanceRead,
    TenantGovernanceManage,
    TenantPreviewCreate,
    TenantPreviewRead,
    TenantDiagnosticsRead,
    TenantSimulationCreate,
    TenantSimulationRead,
    TenantSimulationDiscard,
    BusinessMetricsRead,
    BillingRead,
    BillingManage,
    ContractManage,
    SupportCaseRead,
    SupportCaseManage,
    AuditRead,
    SecurityEventsRead,
    PlatformIdentityManage,
    PlatformRoleManage,
}

pub const ALL_PLATFORM_CAPABILITIES: &[PlatformCapability] = &[
    PlatformCapability::PlatformOverviewRead,
    PlatformCapability::PlatformHealthRead,
    PlatformCapability::PlatformOperationsManage,
    PlatformCapability::TenantList,
    PlatformCapability::TenantRead,
    PlatformCapability::TenantCreate,
    PlatformCapability::TenantUpdate,
    PlatformCapability::TenantSuspend,
    PlatformCapability::TenantDelete,
    PlatformCapability::TenantGovernanceRead,
    PlatformCapability::TenantGovernanceManage,
    PlatformCapability::TenantPreviewCreate,
    PlatformCapability::TenantPreviewRead,
    PlatformCapability::TenantDiagnosticsRead,
    PlatformCapability::TenantSimulationCreate,
    PlatformCapability::TenantSimulationRead,
    PlatformCapability::TenantSimulationDiscard,
    PlatformCapability::BusinessMetricsRead,
    PlatformCapability::BillingRead,
    PlatformCapability::BillingManage,
    PlatformCapability::ContractManage,
    PlatformCapability::SupportCaseRead,
    PlatformCapability::SupportCaseManage,
    PlatformCapability::AuditRead,
    PlatformCapability::SecurityEventsRead,
    PlatformCapability::PlatformIdentityManage,
    PlatformCapability::PlatformRoleManage,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuthorityContext {
    Tenant {
        membership_id: TenantMembershipId,
        tenant_id: crate::TenantId,
        role: TenantRole,
    },
    Platform {
        membership_id: PlatformMembershipId,
        roles: Vec<PlatformRole>,
    },
}

impl AuthorityContext {
    pub fn allows(&self, capability: PlatformCapability) -> bool {
        let Self::Platform { roles, .. } = self else {
            return false;
        };
        roles
            .iter()
            .any(|role| platform_role_allows(*role, capability))
    }
}

pub fn platform_role_allows(role: PlatformRole, capability: PlatformCapability) -> bool {
    use PlatformCapability as C;
    match role {
        PlatformRole::Owner => true,
        PlatformRole::Admin => matches!(
            capability,
            C::PlatformOverviewRead
                | C::TenantList
                | C::TenantRead
                | C::TenantCreate
                | C::TenantUpdate
                | C::TenantSuspend
                | C::TenantGovernanceRead
                | C::TenantGovernanceManage
                | C::PlatformIdentityManage
                | C::PlatformRoleManage
                | C::BillingRead
                | C::BillingManage
                | C::ContractManage
                | C::AuditRead
        ),
        PlatformRole::Operator => matches!(
            capability,
            C::PlatformOverviewRead
                | C::PlatformHealthRead
                | C::PlatformOperationsManage
                | C::TenantList
                | C::TenantRead
                | C::TenantDiagnosticsRead
                | C::AuditRead
                | C::TenantSimulationCreate
                | C::TenantSimulationRead
                | C::TenantSimulationDiscard
        ),
        PlatformRole::SupportEngineer => matches!(
            capability,
            C::TenantList
                | C::TenantRead
                | C::TenantPreviewCreate
                | C::TenantPreviewRead
                | C::TenantDiagnosticsRead
                | C::SupportCaseRead
                | C::SupportCaseManage
        ),
        PlatformRole::BusinessOperator => matches!(
            capability,
            C::PlatformOverviewRead
                | C::TenantList
                | C::TenantRead
                | C::BusinessMetricsRead
                | C::BillingRead
                | C::BillingManage
                | C::ContractManage
        ),
        PlatformRole::SecurityAuditor => matches!(
            capability,
            C::PlatformOverviewRead
                | C::TenantList
                | C::TenantRead
                | C::AuditRead
                | C::SecurityEventsRead
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_have_stable_wire_names() {
        assert_eq!(
            serde_json::to_string(&TenantRole::Owner).unwrap(),
            "\"owner\""
        );
        assert_eq!(
            serde_json::to_string(&PlatformRole::Owner).unwrap(),
            "\"platform_owner\""
        );
        assert_eq!(
            serde_json::to_string(&PlatformRole::Admin).unwrap(),
            "\"platform_admin\""
        );
        assert_eq!(
            serde_json::to_string(&PlatformRole::Operator).unwrap(),
            "\"platform_operator\""
        );
        assert_eq!(
            serde_json::to_string(&PlatformRole::SupportEngineer).unwrap(),
            "\"support_engineer\""
        );
    }

    #[test]
    fn identifiers_reject_blank_values() {
        assert!(IdentityId::new(" ").is_err());
        assert!(PlatformMembershipId::new("").is_err());
        assert!(serde_json::from_str::<IdentityId>("\"  \"").is_err());
    }
}
