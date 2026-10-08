//! R4 tombstone for the retired public Booking HTTP compatibility adapter.
//!
//! `/api/booking/*` is intentionally no longer mounted. Canonical reservation
//! writes and availability live under `reservation_v2`; the remaining internal
//! read-only `booking` module is a bounded compatibility hold until R4-P3
//! supplies the generic Query/Capability replacement for estimate/device search.
//!
//! Keep this file only as structural evidence for the R1-P4 compatibility
//! lineage. Reintroducing public Booking routes is forbidden by R4-P2.
//!
//! Canonical Registry replacement key retained for the historical structural gate:
//! "reservation_v2".
