<!-- TALOS_MACHINE_QUALIFICATION_PROJECTION v1 -->
<!-- source_blob_sha: 9ba840ec4a9c1a21b16ea9ab8dc79fcd0b501af5 -->
<!-- human narrative authority: private talos-knowledge PR #33 -->
<!-- source_path: policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r1-p3-order-query-api-v2.md -->

TALOS-OPS-029
structural_authority: TALOS-OPS-029
6. evidence compare              → TALOS-OPS-029 + exact-head CI
`TALOS-OPS-029` is the deterministic structural authority and carries 9 negative fixtures plus CRLF parity. Exact-head Quality also proves Rust formatting, locked workspace check, full workspace tests, PostgreSQL feature compile, RustSec, frontend typecheck/build, Harness compatibility and production dependency audit.
state_machine::allowed_next
`allowedActions` is derived from `official_order::state_machine::allowed_next`. The frontend is not allowed to maintain a second transition graph. This does not migrate the transition write command itself; R1-P5/O-03 and related owners retain write-policy ownership.
TrustedTenantUser
order_service::
The zero-production-caller `order_service::query_orders_offset` function is deleted. The old raw-pool offset service path must not be reintroduced.
order_query_v2
The one-shot composition/bootstrap and descriptor-fixture workflows were removed before the final candidate. The Registry descriptor catalog contains 45 entries after adding `order_query_v2`, and the deterministic fixture matches that actual catalog size.
→ order_query_v2
1. V2 query                       → order_query_v2 + /api/v2/orders
- Registry descriptor cardinality fixture was synchronized from 44 to 45 after adding `order_query_v2`.
STATUS_TRANSITIONS
The active frontend Order store uses `/api/v2/orders` and page-based fetching only. The old frontend `fetchOffset` API and frontend-owned `STATUS_TRANSITIONS` graph are removed from the canonical read/action path.
allowedActions
allowedActions[]
ORDER_QUERY_V2_SINGLE_READ_AUTHORITY
`ORDER_QUERY_V2_SINGLE_READ_AUTHORITY`
ORDER_V2_PAGE_STRATEGY_ONLY
`ORDER_V2_PAGE_STRATEGY_ONLY`
ORDER_SUMMARY_DETAIL_TYPED
`ORDER_SUMMARY_DETAIL_TYPED`
SERVER_ALLOWED_ACTIONS_REQUIRED
`SERVER_ALLOWED_ACTIONS_REQUIRED`
LEGACY_USERS_READ_COMPATIBILITY_PRESERVED
`LEGACY_USERS_READ_COMPATIBILITY_PRESERVED`
LEGACY_OFFSET_SERVICE_QUERY_REMOVED
`LEGACY_OFFSET_SERVICE_QUERY_REMOVED`
R1-P4
next_boundary: R1-P4
R1-P3 / O-01 is `VERIFIED_MERGED`. The next release-train boundary is `R1-P4 Reservation / Allocation`. Order write strangler O-02 through O-07 remain separately owned and must satisfy their own compatibility/deletion gates before O-08 can remove the legacy service.
R1-P3 Order Query / API V2
# R1-P3 Order Query / API V2
document_id: TALOS-OPS-R1-P3-ORDER-QUERY-API-V2
master_plan_story: SP-03
This story is the named continuation owner from SP-03I for Order V2 query, `/users` sunset preparation and legacy query deletion. It executes O-01 Query without inventing another generic SP-03 foundation slice.
