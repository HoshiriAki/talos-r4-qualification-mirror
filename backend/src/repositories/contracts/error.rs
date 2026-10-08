use std::error::Error;
use std::fmt::{self, Display, Formatter};

#[derive(Debug)]
pub enum RepositoryError {
    TenantScopeRequired,
    ScopeUnresolved,
    SimulationUnsupported,
    PreviewWriteDenied,
    PoolUnavailable(String),
    AdapterUnavailable(String),
    Sqlite(String),
    Postgres(String),
    ContractViolation(String),
}

impl RepositoryError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::TenantScopeRequired => "REPOSITORY_TENANT_SCOPE_REQUIRED",
            Self::ScopeUnresolved => "REPOSITORY_SCOPE_UNRESOLVED",
            Self::SimulationUnsupported => "REPOSITORY_SIMULATION_UNSUPPORTED",
            Self::PreviewWriteDenied => "REPOSITORY_PREVIEW_WRITE_DENIED",
            Self::PoolUnavailable(_) => "REPOSITORY_POOL_UNAVAILABLE",
            Self::AdapterUnavailable(_) => "REPOSITORY_ADAPTER_UNAVAILABLE",
            Self::Sqlite(_) => "REPOSITORY_SQLITE",
            Self::Postgres(_) => "REPOSITORY_POSTGRES",
            Self::ContractViolation(_) => "REPOSITORY_CONTRACT_VIOLATION",
        }
    }
}

impl Display for RepositoryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::PoolUnavailable(detail)
            | Self::AdapterUnavailable(detail)
            | Self::Sqlite(detail)
            | Self::ContractViolation(detail) => {
                write!(formatter, "{}: {detail}", self.code())
            }
            // sqlx database errors can contain SQL, schema, provider or connection
            // details. Preserve the detail in Debug for trusted diagnostics, but
            // never surface it through the stable Display/API representation.
            Self::Postgres(_) => formatter.write_str(self.code()),
            _ => formatter.write_str(self.code()),
        }
    }
}

impl Error for RepositoryError {}
