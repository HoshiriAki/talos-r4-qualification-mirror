mod provider;
mod session;

pub use provider::SqliteRepositoryProvider;
pub(in crate::repositories) use session::SqliteRepositorySession as SqliteSessionBackend;

// Transitional P8-C compatibility alias. Existing repository families can keep
// their historical import while the actual scoped session is backend-neutral.
// Remove this alias after every family imports RepositorySession directly.
pub(in crate::repositories) type SqliteRepositorySession =
    crate::repositories::session::RepositorySession;
