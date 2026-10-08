import assert from 'node:assert/strict';
import { collectSnapshot, validateSnapshot } from './check-r4-platform-security-hardening.mjs';

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectSnapshot());
  mutate(snapshot);
  const errors = validateSnapshot(snapshot);
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`);
  assert.ok(
    errors.some((error) => error.includes(needle)),
    `${name}: expected error containing ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  );
}

const baselineErrors = validateSnapshot(collectSnapshot());
assert.deepEqual(baselineErrors, [], `baseline must pass before mutation tests: ${baselineErrors.join('; ')}`);

expectFailure(
  'auth throttling authority leaks back into AppState',
  (snapshot) => {
    snapshot.state += '\npub auth_rate_limiter: Arc<AuthRateLimiter>,\n';
  },
  'SP-06 AppState must not absorb',
);

expectFailure(
  'router auth limiter stops using the identity pool',
  (snapshot) => {
    snapshot.routesMod = snapshot.routesMod.replace(
      'state.auth_security_repository().clone()',
      'AuthSecurityRepository::new(other_pool.clone())',
    );
  },
  'derive AuthRateLimiter from the AppState AuthSecurityRepository',
);

expectFailure(
  'router stops injecting durable auth limiter extension',
  (snapshot) => {
    snapshot.routesMod = snapshot.routesMod.replace(
      '.layer(Extension(auth_rate_limiter))',
      '.layer(Extension(metrics.clone()))',
    );
  },
  'inject the durable AuthRateLimiter as an HTTP Extension',
);

expectFailure(
  'one auth mutation stops extracting router limiter',
  (snapshot) => {
    snapshot.authRoute = snapshot.authRoute.replace(
      'Extension(auth_rate_limiter): Extension<Arc<AuthRateLimiter>>,',
      '_removed_auth_rate_limiter: Arc<AuthRateLimiter>,',
    );
  },
  'platform login, tenant login and password change',
);

expectFailure(
  'MFA verification stops extracting router limiter',
  (snapshot) => {
    snapshot.complianceRoute = snapshot.complianceRoute.replace(
      'Extension(auth_rate_limiter): Extension<Arc<AuthRateLimiter>>,',
      '_removed_auth_rate_limiter: Arc<AuthRateLimiter>,',
    );
  },
  'MFA verification must extract',
);

expectFailure(
  'production route falls back to legacy memory rate authority',
  (snapshot) => {
    snapshot.authRoute += '\n// auth_service::check_login_rate_limit("ip", "user", &state.config)\n';
  },
  'must not call legacy process-local',
);

expectFailure(
  'raw XFF middleware revival',
  (snapshot) => {
    snapshot.legacyBruteForceExists = true;
  },
  'brute_force.rs must remain retired',
);

expectFailure(
  'middleware export revival',
  (snapshot) => {
    snapshot.middlewareMod += '\npub mod ip_whitelist;\n';
  },
  'must not re-export ip_whitelist',
);

expectFailure(
  'repository rate update loses serialization',
  (snapshot) => {
    snapshot.authRepository = snapshot.authRepository.replaceAll(
      'TransactionBehavior::Immediate',
      'TransactionBehavior::Deferred',
    );
  },
  'serialize durable rate-state mutation',
);

expectFailure(
  'rate service steals SQL write authority',
  (snapshot) => {
    snapshot.authRate += '\n// INSERT INTO auth_rate_limit_state VALUES (...)\n';
  },
  'must not own SQL writes',
);

expectFailure(
  'session service steals SQL write authority',
  (snapshot) => {
    snapshot.sessionSecurity += '\n// INSERT INTO auth_sessions VALUES (...)\n';
  },
  'session security service must not own SQL writes',
);

expectFailure(
  'auth rate policy reinterprets forwarding headers',
  (snapshot) => {
    snapshot.authRate += '\n// x-forwarded-for\n';
  },
  'must not reinterpret forwarding headers',
);

expectFailure(
  'platform login loses configured ttl',
  (snapshot) => {
    snapshot.authRoute = snapshot.authRoute.replace(
      'state.config.session_ttl_days,',
      '7,',
    );
  },
  'platform production login must bind',
);

expectFailure(
  'platform login bypasses repository-aware canonical TOTP authority',
  (snapshot) => {
    snapshot.authRoute = snapshot.authRoute.replace(
      'totp_login::verify_login_totp_with_repository(\n        state.auth_security_repository(),',
      'totp_login::verify_login_totp(\n        &state.pool,',
    );
  },
  'both use canonical TOTP login authority',
);

expectFailure(
  'production route falls back to legacy fixed ttl session helper',
  (snapshot) => {
    snapshot.authRoute += '\n// auth_service::create_session(&state.pool, "identity")\n';
  },
  'must not use legacy fixed-TTL',
);

expectFailure(
  'pre-auth account state enumeration returns',
  (snapshot) => {
    snapshot.authRoute += '\nconst _: &str = "AUTH_ACCOUNT_DISABLED";\n';
  },
  'pre-auth tenant/status failures',
);

expectFailure(
  'TOTP enrollment revives a local verifier',
  (snapshot) => {
    snapshot.twoFa += '\nfn verify_totp(_secret: &[u8], _code: &str) -> bool { true }\n';
  },
  'must not reintroduce a second TOTP verifier',
);

expectFailure(
  'TOTP enrollment bypasses canonical verifier call',
  (snapshot) => {
    snapshot.twoFa = snapshot.twoFa.replace(
      'verify_totp_code(&decrypted.secret, &input.code)',
      'verify_totp(&decrypted.secret, &input.code)',
    );
  },
  'must call the canonical code verifier',
);

expectFailure(
  'production TOTP compatibility bypasses canonical verifier call',
  (snapshot) => {
    snapshot.twoFaCompatibility = snapshot.twoFaCompatibility.replace(
      'verify_totp_code(&decrypted.secret, &input.code)',
      'verify_totp(&decrypted.secret, &input.code)',
    );
  },
  'production TOTP enrollment verification must call the canonical code verifier',
);

expectFailure(
  'TOTP previous-key rotation seam removed',
  (snapshot) => {
    snapshot.totpCrypto = snapshot.totpCrypto.replaceAll(
      'TALOS_TOTP_ENCRYPTION_KEY_PREVIOUS',
      'REMOVED_TOTP_PREVIOUS_KEY',
    );
  },
  'previous-key rotation seam',
);

expectFailure(
  'TOTP login service steals credential SQL authority',
  (snapshot) => {
    snapshot.totpLogin += '\nconst _STOLEN_TOTP_WRITE: &str = "UPDATE identities SET totp_secret_ciphertext = ?1";\n';
  },
  'TOTP login service must not own credential SQL writes',
);

expectFailure(
  'TOTP repository loses verified-envelope CAS predicate',
  (snapshot) => {
    snapshot.authRepository = snapshot.authRepository.replace(
      'totp_secret_ciphertext = ?4',
      'totp_secret_ciphertext = ?5',
    );
  },
  'TOTP rewrap must compare-and-swap',
);

expectFailure(
  'registry payload guard returns to depth-only checking',
  (snapshot) => {
    snapshot.coreLib = snapshot.coreLib.replace(
      '        let mut encoded_bytes = 0usize;\n        self.check_value(payload, 0, &mut encoded_bytes)\n',
      '        self.check_depth(payload, 0)?;\n        Ok(())\n',
    );
  },
  'execute aggregate byte accounting',
);

expectFailure(
  'registry aggregate payload rejection code removed',
  (snapshot) => {
    snapshot.coreLib = snapshot.coreLib.replaceAll(
      'VAL_PAYLOAD_TOO_LARGE',
      'VAL_PAYLOAD_TOO_BIG_REMOVED',
    );
  },
  'stable aggregate-payload rejection code',
);

expectFailure(
  'spreadsheet parser bypasses import admission',
  (snapshot) => {
    snapshot.excelImport = snapshot.excelImport.replace(
      '    validate_import_file(buffer, file_name)?;\n',
      '',
    );
  },
  'import admission gate first',
);

expectFailure(
  'archive expansion gate removed',
  (snapshot) => {
    snapshot.excelImport = snapshot.excelImport.replace(
      '        validate_zip_expansion_budget(buffer)?;\n',
      '',
    );
  },
  'archive expansion admission',
);

expectFailure(
  'workbook row budget removed',
  (snapshot) => {
    snapshot.excelImport = snapshot.excelImport.replaceAll(
      'IMPORT_WORKBOOK_ROWS_MAX',
      'REMOVED_WORKBOOK_ROWS_MAX',
    );
  },
  'cap workbook rows',
);

expectFailure(
  'spreadsheet temp cleanup removed',
  (snapshot) => {
    snapshot.excelImport = snapshot.excelImport.replace(
      '        let _ = std::fs::remove_file(&self.0);\n',
      '',
    );
  },
  'cleaned on all parser exit paths',
);

expectFailure(
  'registry Excel commands lose bounded guard',
  (snapshot) => {
    snapshot.officialExcelImport = snapshot.officialExcelImport.replaceAll(
      'registry_excel_guard().check_raw(&payload)?;',
      'DeserializeGuard::default().check_raw(&payload)?;',
    );
  },
  'parse_excel and validate_excel must both execute',
);

expectFailure(
  'registry Excel archive admission removed',
  (snapshot) => {
    snapshot.officialExcelImport = snapshot.officialExcelImport.replace(
      '        validate_zip_expansion_budget(buffer)?;\n',
      '',
    );
  },
  'ZIP formats must pass archive expansion admission',
);

expectFailure(
  'registry device import row limit removed',
  (snapshot) => {
    snapshot.officialExcelImport = snapshot.officialExcelImport.replaceAll(
      'REGISTRY_DEVICE_IMPORT_ROWS_MAX',
      'REMOVED_DEVICE_IMPORT_ROWS_MAX',
    );
  },
  'cap row-level DB/module amplification',
);

expectFailure(
  'registry Excel temp cleanup removed',
  (snapshot) => {
    snapshot.officialExcelImport = snapshot.officialExcelImport.replace(
      '        let _ = std::fs::remove_file(&self.0);\n',
      '',
    );
  },
  'registry Excel temp files must be cleaned',
);

expectFailure(
  'shared multipart parser stops propagating parser errors',
  (snapshot) => {
    snapshot.multipartImport = snapshot.multipartImport.replace(
      '.next_field()\n            .await\n            .map_err(|error| AppError::BadRequest(format!("multipart 解析失败: {error}")))?',
      '.next_field()\n            .await\n            .ok()\n            .flatten()',
    );
  },
  'parser errors must be propagated',
);

expectFailure(
  'order imports revive swallowed multipart loop',
  (snapshot) => {
    snapshot.orderRoute += '\n// while let Ok(Some(field)) = multipart.next_field().await {}\n';
  },
  'must not swallow multipart parser errors',
);

expectFailure(
  'one order import bypasses shared multipart parser',
  (snapshot) => {
    snapshot.orderRoute = snapshot.orderRoute.replace(
      'multipart_import::parse_excel_import_multipart(&mut multipart, true).await?;',
      'legacy_parse(&mut multipart).await?;',
    );
  },
  'all three order Excel imports',
);

expectFailure(
  'order import execution row budget removed',
  (snapshot) => {
    snapshot.orderRoute = snapshot.orderRoute.replace(
      '    enforce_item_limit("导入行", parsed.rows.len(), ORDER_IMPORT_ROWS_MAX)?;\n',
      '',
    );
  },
  'all three order Excel imports must enforce',
);

expectFailure(
  'order bulk fanout constant removed',
  (snapshot) => {
    snapshot.orderRoute = snapshot.orderRoute.replaceAll(
      'ORDER_BULK_ITEMS_MAX',
      'REMOVED_ORDER_BULK_ITEMS_MAX',
    );
  },
  'explicit bulk fanout budget',
);

expectFailure(
  'batch shipment aggregate accounting removed',
  (snapshot) => {
    snapshot.orderRoute = snapshot.orderRoute.replace(
      '.checked_add(device_count)',
      '.saturating_add(device_count)',
    );
  },
  'aggregate device accounting must fail closed',
);

expectFailure(
  'device import candidates lose SQLite tenant predicate',
  (snapshot) => {
    snapshot.deviceCandidateSqlite = snapshot.deviceCandidateSqlite.replace(
      'WHERE tenant_id = ?1',
      'WHERE 1 = 1',
    );
  },
  'SQLite device import candidates must be selected inside the scoped tenant boundary',
);

expectFailure(
  'device import candidates lose PostgreSQL tenant predicate',
  (snapshot) => {
    snapshot.deviceCandidatePostgres = snapshot.deviceCandidatePostgres.replace(
      'WHERE tenant_id = $1',
      'WHERE 1 = 1',
    );
  },
  'PostgreSQL device import candidates must be selected inside the scoped tenant boundary',
);

expectFailure(
  'order device import stops passing ExecutionContext to candidate facade',
  (snapshot) => {
    snapshot.orderRoute = snapshot.orderRoute.replace(
      '.list_for_import(&ctx)',
      '.list_for_import_unscoped()',
    );
  },
  'bind candidate matching to the authenticated ExecutionContext',
);

expectFailure(
  'order export returns to unscoped legacy reader',
  (snapshot) => {
    snapshot.orderRoute += '\n// order_compatibility_support::query_orders(&state.pool, &filter)?;\n';
  },
  'must not return to unscoped legacy order readers',
);

expectFailure(
  'filtered order export loses total row budget',
  (snapshot) => {
    snapshot.orderExportService = snapshot.orderExportService.replace(
      '    if total > max_rows {',
      '    if false {',
    );
  },
  'reject results above its row budget',
);

expectFailure(
  'filtered order export loses scoped authority on one page read',
  (snapshot) => {
    snapshot.orderExportService = snapshot.orderExportService.replace(
      '            "order_read_compatibility",\n            "list_orders",',
      '            "legacy_order",\n            "list_orders",',
    );
  },
  'for every page',
);

expectFailure(
  'explicit-id order export loses scoped read authority',
  (snapshot) => {
    snapshot.orderExportService = snapshot.orderExportService.replace(
      '                "order_read_compatibility",\n                "get_order",',
      '                "legacy_order",\n                "get_order",',
    );
  },
  'scoped read authority',
);

expectFailure(
  'legacy device list loses bounded scoped authority',
  (snapshot) => {
    snapshot.deviceReadAuthority = snapshot.deviceReadAuthority.replace(
      'pub const DEVICE_LEGACY_LIST_ROWS_MAX: usize = 500;',
      'pub const DEVICE_LEGACY_LIST_ROWS_MAX: usize = 0;',
    );
  },
  'legacy device list must keep a finite row budget',
);

expectFailure(
  'legacy device list restores SQLite-only route helper',
  (snapshot) => {
    snapshot.deviceRoute += '\n// device_bounded_read::list_legacy_devices_bounded\n';
  },
  'must not restore the SQLite-only bounded compatibility helper',
);

expectFailure(
  'device HTTP list returns to unbounded registry command',
  (snapshot) => {
    snapshot.deviceRoute += '\n// registry.execute("device", "list_devices", payload, &ctx)\n';
  },
  'must not return to the unbounded legacy Registry list command',
);

expectFailure(
  'device warning pagination loses scan admission',
  (snapshot) => {
    snapshot.deviceReadAuthority = snapshot.deviceReadAuthority.replace(
      'DEVICE_WARNING_SCAN_ROWS_MAX',
      'REMOVED_DEVICE_WARNING_SCAN_ROWS_MAX',
    );
  },
  'warning-status pagination must keep a finite pre-scan budget',
);

expectFailure(
  'device filtered export loses finite result budget',
  (snapshot) => {
    snapshot.deviceReadAuthority = snapshot.deviceReadAuthority.replace(
      'DEVICE_EXPORT_ROWS_MAX',
      'REMOVED_DEVICE_EXPORT_ROWS_MAX',
    );
  },
  'device export must keep a finite result budget',
);

expectFailure(
  'device filtered export returns to unbounded query',
  (snapshot) => {
    snapshot.deviceRoute += '\n// device_service::query_devices(&state.pool, &tenant.id, &filter)?;\n';
  },
  'must not directly invoke the unbounded compatibility query',
);

expectFailure(
  'device bulk update/delete limits removed',
  (snapshot) => {
    snapshot.deviceRoute = snapshot.deviceRoute.replaceAll(
      'raw.len() > DEVICE_BULK_ITEMS_MAX',
      'false',
    );
  },
  'device bulk delete and update',
);

expectFailure(
  'backup status loses scan limit',
  (snapshot) => {
    snapshot.backupRoute = snapshot.backupRoute.replace(
      'const BACKUP_SCAN_ENTRY_LIMIT: usize = 4096;\n',
      '',
    );
  },
  'finite directory-entry scan budget',
);

expectFailure(
  'backup status returns to unbounded collect',
  (snapshot) => {
    snapshot.backupRoute += '\n// read_dir(backup_dir).unwrap().collect::<Vec<_>>()\n';
  },
  'must not collect an unbounded directory',
);

expectFailure(
  'backend audit loses recursive secret redaction',
  (snapshot) => {
    snapshot.auditService = snapshot.auditService.replace(
      'fn redact_audit_value(',
      'fn removed_redact_audit_value(',
    );
  },
  'recursively sanitize audit detail',
);

expectFailure(
  'backend audit stops applying opaque reference classifier',
  (snapshot) => {
    snapshot.auditService = snapshot.auditService.replace(
      'let opaque_reference = is_opaque_secret_reference_key(&key);',
      'let opaque_reference = false;',
    );
  },
  'apply the opaque-reference classifier',
);

expectFailure(
  'feature audit stops redacting credential material',
  (snapshot) => {
    snapshot.featureAudit = snapshot.featureAudit.replace(
      'let credential_material = key.contains("credential") && !opaque_reference;',
      'let credential_material = false;',
    );
  },
  'redact credential values/material',
);

expectFailure(
  'feature audit restores plaintext oversized preview',
  (snapshot) => {
    snapshot.featureAudit += '\n// { "preview": detail_json }\n';
  },
  'must not persist a plaintext preview',
);

expectFailure(
  'registry execution audit stores payload again',
  (snapshot) => {
    snapshot.registrySource = snapshot.registrySource.replaceAll(
      'AuditPayloadPolicy::ReferenceOnly',
      'AuditPayloadPolicy::FullPayload',
    );
  },
  'must stay reference-only',
);

expectFailure(
  'internal AppError logging restores raw Display text',
  (snapshot) => {
    snapshot.errorSource += '\n// tracing::error!("Internal error: {}", self.to_string());\n';
  },
  'must not log raw Display details',
);

expectFailure(
  'tenant preview logs raw module message',
  (snapshot) => {
    snapshot.tenantPreview = snapshot.tenantPreview.replace(
      'tracing::error!(code = %payload.code, "tenant preview command failed");',
      'tracing::error!(code = %payload.code, message = %payload.message, "tenant preview command failed");',
    );
  },
  'must not log raw module error messages',
);

expectFailure(
  'tenant preview secret-bearing regression test removed',
  (snapshot) => {
    snapshot.tenantPreview = snapshot.tenantPreview.replace(
      'fn unknown_preview_error_discards_module_message()',
      'fn removed_preview_error_test()',
    );
  },
  'negative regression test',
);

expectFailure(
  'migration composition forgets sqlite 069',
  (snapshot) => {
    snapshot.dbMod = snapshot.dbMod.replace('executed.extend(r4_migrations::run_sqlite_extension_069(conn)?);', '');
  },
  'SQLite migration composition',
);

expectFailure(
  'migration persists raw username',
  (snapshot) => {
    snapshot.sqliteMigration += '\nALTER TABLE auth_rate_limit_state ADD COLUMN username TEXT;\n';
  },
  'must not persist raw IP or username',
);

console.log('R4-P7 platform security hardening mutation suite PASS');
