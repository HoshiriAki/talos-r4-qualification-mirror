<!-- TALOS_MACHINE_QUALIFICATION_PROJECTION v1 -->
<!-- source_blob_sha: c21fa515c0909548faa9f4d881e46010347c5cd5 -->
<!-- human narrative authority: private talos-knowledge PR #33 -->
<!-- source_path: policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r1-p2-quote-pricing-orderline.md -->

name
A model snapshot includes the Pricing authority name, normalized pricing input and full server result including shipping/breakdown fields. An accessory snapshot includes catalog identity, SKU, version and server unit price.
SimulationSupport::Blocked
Preview may read Quote state through the same scoped repository. Database writes are denied before the module handler. Quote remains `SimulationSupport::Blocked`, so Simulation continues to fail closed until R3 supplies repository-backed Simulation semantics.
TrustedTenantUser
TrustedTenantUser/Admin
R1-P2
document_id: TALOS-OPS-R1-P2-QUOTE-PRICING-ORDERLINE
release_train_story: R1-P2
# R1-P2 Quote / Pricing V2 / OrderLine
R1-P2 deliberately does not implement a second model-price calculator. The existing Pricing module remains the calculation authority while its output is materialized into the new Quote contract.
Cancellation remains outside this bounded R1-P2 command surface; expiry is executable and cannot be forced before its server deadline.
`POST /users` is no longer mounted as an Order creation surface. The frontend `createOrder(data: Partial<Order>)` adapter and store `create(Partial<Order>)` method are removed. Legacy `/users` reads remain intentionally owned by R1-P3 compatibility migration; legacy update/delete and other write operations are outside this R1-P2 create-path retirement.
Full PostgreSQL repository/runtime parity remains R3-P5; R1-P2 supplies schema parity and continues to pass the PostgreSQL feature compile Gate.
R1-P2 is `VERIFIED_MERGED`. The next release-train boundary is `R1-P3 Order Query/API V2`.
TALOS-OPS-028
structural_authority: TALOS-OPS-028
`TALOS-OPS-028` is the structural authority for this story and carries negative fixtures for client price authority, float persistence, missing snapshot immutability, Registry bypass, device allocation leakage, migration parity and the composite Order parent-key contract.
PRICEPAGE_QUOTE_V2_CUTOVER
`PRICEPAGE_QUOTE_V2_CUTOVER`
accessory_catalog
- `accessory_catalog`
quotes
→ POST /api/v2/quotes
GET  /api/v2/quotes
POST /api/v2/quotes
GET  /api/v2/quotes/{id}
POST /api/v2/quotes/{id}/confirm
POST /api/v2/quotes/{id}/expire
POST /api/v2/quotes/{id}/orders
- `quotes`
quote_lines
- `quote_lines`
order_lines
- `order_lines`
- an exact `(orders.id, tenant_id)` composite parent key required by `order_lines(order_id, tenant_id)` on both database profiles.
source_quote_id
The `orders(tenant_id, source_quote_id)` unique index plus Quote conversion state prevent one Quote from creating multiple Orders.
- `orders.source_quote_id`
total_minor
- `orders.total_minor`
unit_price_minor
→ server unit_price_minor
price_snapshot_json
→ immutable QuoteLine.price_snapshot_json
/api/v2/quotes/{id}/expire
SERVER_PRICE_AUTHORITY_REQUIRED
`SERVER_PRICE_AUTHORITY_REQUIRED`
CLIENT_TOTAL_FORBIDDEN
`CLIENT_TOTAL_FORBIDDEN`
MONEY_MINOR_UNITS_REQUIRED
`MONEY_MINOR_UNITS_REQUIRED`
QUOTE_PRICE_SNAPSHOT_IMMUTABLE
`QUOTE_PRICE_SNAPSHOT_IMMUTABLE`
MULTI_LINE_QUOTE_REQUIRED
`MULTI_LINE_QUOTE_REQUIRED`
ACCESSORY_CATALOG_PRICE_REQUIRED
`ACCESSORY_CATALOG_PRICE_REQUIRED`
SERVER_ORDER_NUMBER_REQUIRED
`SERVER_ORDER_NUMBER_REQUIRED`
NO_DEVICE_ALLOCATION_IN_QUOTE_OR_ORDER_CREATION
`NO_DEVICE_ALLOCATION_IN_QUOTE_OR_ORDER_CREATION`
CREATE_ORDER_FROM_CONFIRMED_QUOTE_ONLY
`CREATE_ORDER_FROM_CONFIRMED_QUOTE_ONLY`
LEGACY_PRICING_CALCULATOR_SINGLE_AUTHORITY
`LEGACY_PRICING_CALCULATOR_SINGLE_AUTHORITY`
LEGACY_DIRECT_ORDER_CREATE_RETIRED
`LEGACY_DIRECT_ORDER_CREATE_RETIRED`
R1-P3
next_boundary: R1-P3
R1-P2 Quote
master_plan_story: SP-06-SP-07
