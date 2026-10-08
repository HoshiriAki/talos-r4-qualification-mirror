<!-- TALOS_MACHINE_QUALIFICATION_PROJECTION v1 -->
<!-- source_blob_sha: 8c3e66353f0b752f11ed75edc4b0dedf6177bef8 -->
<!-- human narrative authority: private talos-knowledge PR #33 -->
<!-- source_path: policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r1-p1-customer-typed-ids.md -->

TALOS-OPS-027
structural_authority: TALOS-OPS-027
TALOS-OPS-027 is the deterministic structural authority for this story and carries negative fixtures for missing scope, missing masking, unsafe legacy inference, Registry bypass and missing migration parity.
CustomerId
├─ CustomerId (UUID)
resolve to an existing/new CustomerId
- preserves the stable `CustomerId` so financial/audit associations remain referentially intact;
phone
Legacy rows that only contain free-text customer name/phone are ambiguous. Migration 053 therefore does not synthesize Customer records or silently merge people from those values.
3. phone/email normalization and PII masking;
name
├─ legal_name
├─ display_name
Phone, name, OpenID, payment account and Provider subject are attributes or external identities. They are not Customer primary keys.
No request-controlled table name is executed as SQL.
- replaces legal/display names with an anonymous label;
customer_contacts
- `customer_contacts`
customer_external_identities
- `customer_external_identities`
customer_history
customer_history receives resolution evidence
- `customer_history`
customer_migration_exceptions
Instead it creates `customer_migration_exceptions` entries for legacy Order/Credit/Overdue/Contract associations that lack a stable Customer reference.
- `customer_migration_exceptions`
customer_id
source row receives customer_id
- nullable `customer_id` associations on the legacy Order/Credit/Overdue/Contract surfaces.
8. source-row `customer_id` association after approved resolution;
SimulationSupport::Blocked
Migration exception resolution and anonymization require `TenantAdmin` authority. All Customer commands are currently `SimulationSupport::Blocked`.
customer
branch: agent/r1-p1-customer-typed-ids
GET    /api/v2/customers
POST   /api/v2/customers
GET    /api/v2/customers/{id}
DELETE /api/v2/customers/{id}                      # anonymize
POST   /api/v2/customers/{id}/contacts
POST   /api/v2/customers/{id}/external-identities
POST   /api/v2/customers/duplicate-candidates
GET    /api/v2/customers/migration-exceptions
POST   /api/v2/customers/migration-exceptions/{id}/resolve
`customer` exposes:
- `list_customers`
- `get_customer`
- `create_customer`
- `anonymize_customer`
`053_customer_domain` is registered in both SQLite and PostgreSQL migration registries.
- `customers`
TrustedTenantUser
All routes use `TrustedTenantUser` or `TrustedTenantAdmin`; none reconstruct tenant scope from request body/header values.
TrustedTenantAdmin
CUSTOMER_DOMAIN_IMPLEMENTED
`CUSTOMER_DOMAIN_IMPLEMENTED` is final for the R1-P1 boundary. `EXACT_HEAD_CI_VERIFIED` and `MERGED_TO_PROTOTYPE` are also final: candidate head `29485187552c114e573dd08979357eea61722700` passed TALOS Repository Preflight run `31198502217` (#71) and TALOS Repository Quality run `31198502253` (#370), then squash-merged through PR #32 as `72c16ac3a592fc1771ad1d4ec4f0242ee7409cac`.
CUSTOMER_ID_IS_BUSINESS_AUTHORITY
`CUSTOMER_ID_IS_BUSINESS_AUTHORITY`
IDENTITY_CUSTOMER_SEPARATION_PRESERVED
`IDENTITY_CUSTOMER_SEPARATION_PRESERVED`
LEGACY_CUSTOMER_INFERENCE_FORBIDDEN
`LEGACY_CUSTOMER_INFERENCE_FORBIDDEN`
SCOPED_CUSTOMER_REPOSITORY_REQUIRED
`SCOPED_CUSTOMER_REPOSITORY_REQUIRED`
PII_MASKING_REQUIRED
`PII_MASKING_REQUIRED`
DUPLICATE_IS_CANDIDATE_NOT_AUTO_MERGE
`DUPLICATE_IS_CANDIDATE_NOT_AUTO_MERGE`
R1-P2
next_boundary: R1-P2
R1-P1 is `VERIFIED_MERGED`. The next release-train boundary is `R1-P2 Quote、Pricing 与 OrderLine`.
R1-P1 Customer
# R1-P1 Customer 与 Typed IDs
R1-P2 Quote
document_id: TALOS-OPS-R1-P1-CUSTOMER-TYPED-IDS
master_plan_story: SP-04-SP-05
