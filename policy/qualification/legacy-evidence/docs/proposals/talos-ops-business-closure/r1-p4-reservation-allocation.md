<!-- TALOS_MACHINE_QUALIFICATION_PROJECTION v1 -->
<!-- source_blob_sha: 6894db6173be2f9075328f0740a4e986bfeff760 -->
<!-- human narrative authority: private talos-knowledge PR #33 -->
<!-- source_path: policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r1-p4-reservation-allocation.md -->

TALOS-OPS-030
structural_authority: TALOS-OPS-030
`TALOS-OPS-030` is the deterministic structural authority and carries negative fixtures for raw-pool canonical writes, inclusive interval regression, client device binding at Reservation creation, missing migration parity, Booking write-authority reintroduction, missing `BEGIN IMMEDIATE`, missing overlap guards, and Registry/route bypass.
The SP-08 successor-delegation wrapper requires TALOS-OPS-027, TALOS-OPS-028 and TALOS-OPS-030 to pass before delegating Customer, Quote and Reservation repositories beyond SP-08's original foundation scope.
- TALOS-OPS-030 half-open repository/migration evidence-token mismatch;
ReservationId
Canonical `ReservationId`, `ReservationRequirementId` and `AllocationId` are UUID-backed typed identifiers. Legacy integer booking/reservation identifiers are never promoted to canonical IDs.
ReservationRequirementId
AllocationId
RepositoryProvider
→ RepositoryProvider.bind(ExecutionContext)
expire_due
A `hold` carries `expires_at`. `expire_due` changes only due holds to `expired`; a confirmed Reservation is not silently expired by the hold expiry path.
rental_reservations
The clean-database migration completion fixture now treats migration 055 as the terminal migration and verifies the four canonical tables `rental_reservations`, `reservation_requirements`, `allocations`, and `reservation_migration_exceptions` exist.
reservation_requirements
allocations
POST /api/v2/reservations/{id}/allocations
POST /api/v2/reservations/allocations/{id}/release
reservation_migration_exceptions
Migration 055 never synthesizes canonical UUID Reservation identities from those rows. Instead it creates tenant-scoped `reservation_migration_exceptions` for explicit later resolution.
055_reservation_allocation_v2
Migration `055_reservation_allocation_v2` is registered for both SQLite and PostgreSQL. This story supplies schema/contract parity only; full PostgreSQL repository/runtime parity remains R3-P5.
reservation_v2
→ reservation_v2
During the compatibility window, legacy `/api/booking/reserve` and `/api/booking/confirm` delegate to `reservation_v2`. The legacy Booking module retains bounded read-only estimate/search compatibility but no longer advertises its old raw-pool Reservation writes through Registry command metadata. Legacy availability uses canonical Reservation capacity semantics rather than per-day inclusive Booking occupancy.
RESERVATION_SINGLE_WRITE_AUTHORITY
`RESERVATION_SINGLE_WRITE_AUTHORITY`
RESERVATION_UUID_REQUIRED
`RESERVATION_UUID_REQUIRED`
HALF_OPEN_INTERVAL_REQUIRED
`HALF_OPEN_INTERVAL_REQUIRED`
MODEL_CAPACITY_RESERVATION_REQUIRED
`MODEL_CAPACITY_RESERVATION_REQUIRED`
RESERVATION_TRANSACTION_SERIALIZATION_REQUIRED
`RESERVATION_TRANSACTION_SERIALIZATION_REQUIRED`
ALLOCATION_OVERLAP_GUARD_REQUIRED
`ALLOCATION_OVERLAP_GUARD_REQUIRED`
RESERVATION_EXPIRY_REQUIRED
`RESERVATION_EXPIRY_REQUIRED`
LEGACY_RESERVATION_INFERENCE_FORBIDDEN
`LEGACY_RESERVATION_INFERENCE_FORBIDDEN`
BOOKING_WRITE_COMPATIBILITY_ADAPTER_ONLY
`BOOKING_WRITE_COMPATIBILITY_ADAPTER_ONLY`
R1-P4 Reservation / Allocation
# R1-P4 Reservation / Allocation
document_id: TALOS-OPS-R1-P4-RESERVATION-ALLOCATION
master_plan_story: SP-03
R1-P4 is the named SP-03I deferred owner after verified R1-P3/O-01. It replaces the previous split Reservation/Booking write authority with one canonical V2 path while preserving bounded one-version read/compatibility behavior where required.
- SP-08 historical deferred-domain matcher via explicit R1-P4 successor delegation;
