import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const read = (root, relativePath) => fs.readFileSync(path.join(root, relativePath), 'utf8');
const exists = (root, relativePath) => fs.existsSync(path.join(root, relativePath));

export function collectSnapshot(root = ROOT) {
  return {
    authService: read(root, 'backend/src/services/auth_service.rs'),
    authRate: read(root, 'backend/src/services/auth_rate_limit.rs'),
    sessionSecurity: read(root, 'backend/src/services/session_security.rs'),
    totpLogin: read(root, 'backend/src/services/totp_login.rs'),
    excelImport: read(root, 'backend/src/services/excel_import_service.rs'),
    officialExcelImport: read(root, 'backend/official/device/src/excel_import.rs'),
    multipartImport: read(root, 'backend/src/services/multipart_import.rs'),
    orderRoute: read(root, 'backend/src/routes/orders.rs'),
    deviceRoute: read(root, 'backend/src/routes/devices.rs'),
    deviceService: read(root, 'backend/src/services/device_service.rs'),
    deviceBoundedRead: read(root, 'backend/src/services/device_bounded_read.rs'),
    deviceReadAuthority: read(root, 'backend/src/application/device_read_authority.rs'),
    deviceCandidateApp: read(root, 'backend/src/application/device_candidates.rs'),
    deviceCandidateSqlite: read(root, 'backend/src/repositories/device_candidate.rs'),
    deviceCandidateDispatch: read(root, 'backend/src/repositories/device_candidate_dispatch.rs'),
    deviceCandidatePostgres: read(root, 'backend/src/repositories/device_candidate_postgres.rs'),
    orderExportService: read(root, 'backend/src/services/order_export_service.rs'),
    backupRoute: read(root, 'backend/src/routes/backup.rs'),
    coreLib: read(root, 'backend/system/core/src/lib.rs'),
    totpCrypto: read(root, 'backend/system/admin/src/totp_crypto.rs'),
    twoFa: read(root, 'backend/system/admin/src/two_fa.rs'),
    twoFaCompatibility: read(root, 'backend/src/application/two_fa_compatibility.rs'),
    auditService: read(root, 'backend/src/services/audit_service.rs'),
    featureAudit: read(root, 'backend/system/admin/src/audit.rs'),
    registrySource: read(root, 'backend/src/registry/mod.rs'),
    errorSource: read(root, 'backend/src/error.rs'),
    tenantPreview: read(root, 'backend/src/routes/tenant_preview.rs'),
    authRepository: read(root, 'backend/src/repositories/auth_security.rs'),
    authRoute: read(root, 'backend/src/routes/auth.rs'),
    complianceRoute: read(root, 'backend/src/routes/compliance.rs'),
    routesMod: read(root, 'backend/src/routes/mod.rs'),
    state: read(root, 'backend/src/state.rs'),
    middlewareMod: read(root, 'backend/src/middleware/mod.rs'),
    r4Migrations: read(root, 'backend/src/db/r4_migrations.rs'),
    dbMod: read(root, 'backend/src/db/mod.rs'),
    sqliteMigration: read(root, 'backend/src/db/migrations/069_r4_platform_security_hardening.sql'),
    pgMigration: read(root, 'backend/src/db/migrations/postgres/069_r4_platform_security_hardening.sql'),
    sqlitePasswordRevocation: read(root, 'backend/src/db/migrations/070_r4_session_revocation_invariant.sql'),
    pgPasswordRevocation: read(root, 'backend/src/db/migrations/postgres/070_r4_session_revocation_invariant.sql'),
    legacyBruteForceExists: exists(root, 'backend/src/middleware/brute_force.rs'),
    legacyIpWhitelistExists: exists(root, 'backend/src/middleware/ip_whitelist.rs'),
  };
}

function requireMatch(errors, source, pattern, message) {
  if (!pattern.test(source)) errors.push(message);
}

function forbidMatch(errors, source, pattern, message) {
  if (pattern.test(source)) errors.push(message);
}

function skipQuoted(source, start, quote) {
  let i = start + 1;
  while (i < source.length) {
    if (source[i] === '\\') {
      i += 2;
      continue;
    }
    if (source[i] === quote) return i + 1;
    i += 1;
  }
  return source.length;
}

function matchingRustBrace(source, openIndex) {
  let depth = 0;
  let i = openIndex;
  while (i < source.length) {
    if (source.startsWith('//', i)) {
      const newline = source.indexOf('\n', i + 2);
      i = newline === -1 ? source.length : newline + 1;
      continue;
    }
    if (source.startsWith('/*', i)) {
      const end = source.indexOf('*/', i + 2);
      if (end === -1) return source.length - 1;
      i = end + 2;
      continue;
    }
    const ch = source[i];
    if (ch === '"') {
      i = skipQuoted(source, i, ch);
      continue;
    }
    if (ch === '{') depth += 1;
    if (ch === '}') {
      depth -= 1;
      if (depth === 0) return i;
    }
    i += 1;
  }
  return source.length - 1;
}

function rustFunctionSource(source, signaturePattern) {
  const match = signaturePattern.exec(source);
  if (!match) return '';
  const open = source.indexOf('{', match.index);
  if (open === -1) return '';
  const close = matchingRustBrace(source, open);
  return source.slice(match.index, close + 1);
}

function rustRuntimeSource(source) {
  const marker = /#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]\s*mod\s+tests\s*\{/g;
  let output = source;
  while (true) {
    marker.lastIndex = 0;
    const match = marker.exec(output);
    if (!match) return output;
    const open = output.indexOf('{', match.index);
    const close = matchingRustBrace(output, open);
    output = output.slice(0, match.index) + output.slice(close + 1);
  }
}

function executableSql(source) {
  return source
    .replace(/--[^\n\r]*/g, ' ')
    .replace(/\/\*[\s\S]*?\*\//g, ' ');
}

const WRITE_SQL = /\b(?:INSERT(?:\s+OR\s+REPLACE)?\s+INTO|UPDATE\s+[A-Za-z_][A-Za-z0-9_]*|DELETE\s+FROM|REPLACE\s+INTO)\b/i;
const RETIRED_RATE_CALL = /auth_service::(?:check_login_rate_limit|record_login_failure|record_login_success|check_custom_rate_limit|record_custom_failure|consume_custom_budget|record_custom_success)/;
const SWALLOWED_MULTIPART = /while\s+let\s+Ok\s*\(\s*Some\s*\(\s*field\s*\)\s*\)\s*=\s*multipart\.next_field\(\)\.await/;

export function validateSnapshot(snapshot) {
  const errors = [];

  if (snapshot.legacyBruteForceExists) {
    errors.push('legacy brute_force.rs must remain retired; P4 ResolvedClientIp is the sole client-IP authority');
  }
  if (snapshot.legacyIpWhitelistExists) {
    errors.push('legacy ip_whitelist.rs must remain retired; raw X-Forwarded-For authority must not return');
  }
  forbidMatch(errors, snapshot.middlewareMod, /pub\s+mod\s+brute_force\s*;/, 'middleware module must not re-export brute_force');
  forbidMatch(errors, snapshot.middlewareMod, /pub\s+mod\s+ip_whitelist\s*;/, 'middleware module must not re-export ip_whitelist');

  const authRateRuntime = rustRuntimeSource(snapshot.authRate);
  requireMatch(errors, authRateRuntime, /AuthSecurityRepository/, 'AuthRateLimiter must delegate persistence to AuthSecurityRepository');
  requireMatch(errors, authRateRuntime, /rate_key_digest\(/, 'auth rate policy must digest logical keys before repository access');
  requireMatch(errors, authRateRuntime, /Sha256/, 'auth rate key digest must remain SHA-256 based');
  forbidMatch(errors, authRateRuntime, WRITE_SQL, 'AuthRateLimiter service must not own SQL writes');
  forbidMatch(errors, authRateRuntime, /TransactionBehavior::Immediate/, 'AuthRateLimiter service must not own database transaction behavior');
  forbidMatch(errors, authRateRuntime, /x-forwarded-for|x-real-ip/i, 'auth rate policy must not reinterpret forwarding headers');

  const sessionSecurityRuntime = rustRuntimeSource(snapshot.sessionSecurity);
  requireMatch(errors, sessionSecurityRuntime, /AuthSecurityRepository/, 'session security service must delegate persistence to AuthSecurityRepository');
  requireMatch(errors, sessionSecurityRuntime, /ttl_days:\s*i64/, 'session security service must require explicit ttl_days');
  requireMatch(errors, sessionSecurityRuntime, /session_expires_at\(ttl_days\)/, 'session security service must derive expiry from explicit ttl_days');
  requireMatch(errors, sessionSecurityRuntime, /Sha256/, 'session bearer persistence must hash tokens before repository insertion');
  forbidMatch(errors, sessionSecurityRuntime, WRITE_SQL, 'session security service must not own SQL writes');
  requireMatch(errors, sessionSecurityRuntime, /rotate_password_and_revoke_sessions/, 'session security must expose atomic credential rotation');

  requireMatch(errors, snapshot.authRepository, /TransactionBehavior::Immediate/, 'auth security repository must serialize durable rate-state mutation');
  requireMatch(errors, snapshot.authRepository, /auth_rate_limit_state/, 'auth security repository must own durable rate-state SQL');
  requireMatch(errors, snapshot.authRepository, /INSERT\s+INTO\s+auth_sessions/i, 'auth security repository must own P7 session insertion');
  requireMatch(errors, snapshot.authRepository, /rotate_password_and_revoke_sessions[\s\S]*?DELETE FROM auth_sessions[\s\S]*?UPDATE identities/, 'credential rotation repository must revoke sessions and update password in one immediate transaction');

  forbidMatch(errors, snapshot.state, /auth_rate_limiter|AuthRateLimiter/, 'SP-06 AppState must not absorb the HTTP ingress auth throttling authority');
  requireMatch(errors, snapshot.routesMod, /use\s+crate::services::auth_rate_limit::AuthRateLimiter\s*;/, 'router composition must import the durable AuthRateLimiter');
  requireMatch(errors, snapshot.routesMod, /let\s+auth_rate_limiter\s*=\s*Arc::new\(AuthRateLimiter::from_repository\(\s*state\.auth_security_repository\(\)\.clone\(\),?\s*\)\)\s*;/, 'router composition must derive AuthRateLimiter from the AppState AuthSecurityRepository');
  requireMatch(errors, snapshot.routesMod, /\.layer\(Extension\(auth_rate_limiter\)\)/, 'router composition must inject the durable AuthRateLimiter as an HTTP Extension');

  const authLimiterExtractors = snapshot.authRoute.match(/Extension\(auth_rate_limiter\):\s*Extension<Arc<AuthRateLimiter>>/g) ?? [];
  if (authLimiterExtractors.length < 3) {
    errors.push('platform login, tenant login and password change must extract the router-owned durable AuthRateLimiter');
  }
  forbidMatch(errors, snapshot.authRoute, RETIRED_RATE_CALL, 'auth routes must not call legacy process-local auth_service rate functions');
  forbidMatch(errors, snapshot.complianceRoute, RETIRED_RATE_CALL, 'MFA routes must not call legacy process-local auth_service rate functions');
  requireMatch(errors, snapshot.complianceRoute, /Extension\(auth_rate_limiter\):\s*Extension<Arc<AuthRateLimiter>>/, 'MFA verification must extract the router-owned durable AuthRateLimiter');

  requireMatch(
    errors,
    snapshot.authRoute,
    /async\s+fn\s+platform_login\s*\([\s\S]*?session_security::create_session_with_repository\(\s*state\.auth_security_repository\(\),\s*&identity\.id,\s*auth_strength,\s*state\.config\.session_ttl_days,\s*\)\?;/,
    'platform production login must bind DB session expiry to state.config.session_ttl_days',
  );
  requireMatch(
    errors,
    snapshot.authRoute,
    /async\s+fn\s+login\s*\([\s\S]*?session_security::create_session_with_repository\(\s*state\.auth_security_repository\(\),\s*&user\.id,\s*auth_strength,\s*state\.config\.session_ttl_days,\s*\)\?;/,
    'tenant production login must bind DB session expiry to state.config.session_ttl_days',
  );
  forbidMatch(errors, snapshot.authRoute, /auth_service::create_session(?:_with_strength)?\s*\(/, 'production auth routes must not use legacy fixed-TTL session creation');
  forbidMatch(errors, snapshot.authRoute, /AUTH_TENANT_MISMATCH|AUTH_ACCOUNT_DISABLED/, 'pre-auth tenant/status failures must stay collapsed into AUTH_INVALID_CREDENTIALS');
  requireMatch(errors, snapshot.authRoute, /ResolvedClientIp/, 'auth routes must consume the P4 trusted ResolvedClientIp projection');
  const canonicalTotpCalls = snapshot.authRoute.match(/totp_login::verify_login_totp_with_repository\s*\(\s*state\.auth_security_repository\(\),/g) ?? [];
  if (canonicalTotpCalls.length < 2) {
    errors.push('tenant and platform production login must both use canonical TOTP login authority');
  }
  forbidMatch(errors, snapshot.authRoute, /auth_service::verify_login_totp\s*\(/, 'production auth routes must not use the legacy divergent TOTP envelope parser');
  requireMatch(errors, snapshot.authRoute, /#\[serde\(deny_unknown_fields\)\][\s\S]{0,80}?struct\s+LoginBody/, 'login input must reject caller-supplied unknown session/token authority');
  requireMatch(errors, snapshot.authRoute, /session_security::rotate_password_and_revoke_sessions/, 'password-change route must use atomic credential/session rotation');
  forbidMatch(errors, snapshot.authRoute, /let\s+_\s*=\s*auth_service::delete_sessions_by_user_id/, 'password-change route must not rely on best-effort session revocation');

  const totpLoginRuntime = rustRuntimeSource(snapshot.totpLogin);
  requireMatch(errors, totpLoginRuntime, /AuthSecurityRepository/, 'production TOTP login must delegate credential persistence to AuthSecurityRepository');
  requireMatch(errors, totpLoginRuntime, /\.with_totp_credential\(/, 'production TOTP login must execute through the repository-owned credential transaction');
  forbidMatch(errors, totpLoginRuntime, WRITE_SQL, 'production TOTP login service must not own credential SQL writes');
  forbidMatch(errors, totpLoginRuntime, /TransactionBehavior::Immediate/, 'production TOTP login service must not own credential transaction behavior');
  requireMatch(errors, totpLoginRuntime, /system_admin::totp_crypto::decrypt_totp_secret/, 'production TOTP login must use canonical envelope crypto');
  requireMatch(errors, totpLoginRuntime, /system_admin::totp_crypto::verify_totp_code/, 'production TOTP login must use canonical code verification');
  requireMatch(errors, totpLoginRuntime, /decrypted\.needs_rewrap/, 'production TOTP login must support staged previous-key rewrap');
  const totpCredentialRepository = rustFunctionSource(
    snapshot.authRepository,
    /pub\(crate\)\s+fn\s+with_totp_credential/,
  );
  requireMatch(errors, totpCredentialRepository, /transaction_with_behavior\(TransactionBehavior::Immediate\)/, 'auth repository must serialize TOTP credential read/rewrap in one immediate transaction');
  requireMatch(errors, totpCredentialRepository, /SELECT\s+totp_enabled,\s*totp_secret_ciphertext\s+FROM\s+identities/i, 'auth repository must own the TOTP credential read');
  requireMatch(errors, totpCredentialRepository, /UPDATE\s+identities[\s\S]*?totp_secret_ciphertext\s*=\s*\?1[\s\S]*?totp_enabled\s*=\s*1[\s\S]*?totp_secret_ciphertext\s*=\s*\?4/i, 'auth repository TOTP rewrap must compare-and-swap the envelope it verified');

  requireMatch(errors, snapshot.totpCrypto, /TALOS_TOTP_ENCRYPTION_KEY_PREVIOUS/, 'TOTP crypto must retain an explicit previous-key rotation seam');
  requireMatch(errors, snapshot.totpCrypto, /needs_rewrap:\s*bool/, 'TOTP decrypt result must report previous-key rewrap requirement');
  requireMatch(errors, snapshot.totpCrypto, /split_once\(':\'\)/, 'TOTP crypto must accept the historical split v1 envelope during migration');
  requireMatch(errors, snapshot.totpCrypto, /strip_prefix\("v1:"\)/, 'TOTP crypto must retain versioned envelope parsing');
  requireMatch(errors, snapshot.totpCrypto, /pub\s+fn\s+verify_totp_code\(/, 'TOTP code verification must have one exported canonical authority');
  requireMatch(errors, snapshot.totpCrypto, /pub\s+fn\s+generate_totp_secret\(/, 'TOTP secret generation must have one exported canonical authority');
  requireMatch(errors, snapshot.totpCrypto, /pub\s+fn\s+base32_encode\(/, 'TOTP base32 encoding must have one exported canonical authority');
  for (const token of [
    'decrypt_totp_secret',
    'encrypt_totp_secret',
    'verify_totp_code',
    'generate_totp_secret',
    'base32_encode',
  ]) {
    requireMatch(
      errors,
      snapshot.twoFa,
      new RegExp('crate::totp_crypto::[\\s\\S]{0,220}\\b' + token + '\\b'),
      `legacy TOTP module must continue using canonical crypto authority: ${token}`,
    );
    requireMatch(
      errors,
      snapshot.twoFaCompatibility,
      new RegExp('system_admin::totp_crypto::[\\s\\S]{0,260}\\b' + token + '\\b'),
      `production TOTP compatibility must use canonical crypto authority: ${token}`,
    );
  }
  requireMatch(errors, snapshot.twoFa, /verify_totp_code\(&decrypted\.secret,\s*&input\.code\)/, 'legacy TOTP enrollment verification must call the canonical code verifier');
  requireMatch(errors, snapshot.twoFaCompatibility, /verify_totp_code\(&decrypted\.secret,\s*&input\.code\)/, 'production TOTP enrollment verification must call the canonical code verifier');
  forbidMatch(errors, snapshot.twoFa, /fn\s+hmac_sha256\s*\(/, 'legacy TOTP module must not reintroduce a second HMAC implementation');
  forbidMatch(errors, snapshot.twoFa, /fn\s+verify_totp\s*\(/, 'legacy TOTP module must not reintroduce a second TOTP verifier');
  forbidMatch(errors, snapshot.twoFaCompatibility, /fn\s+hmac_sha256\s*\(/, 'production TOTP compatibility must not reintroduce a second HMAC implementation');
  forbidMatch(errors, snapshot.twoFaCompatibility, /fn\s+verify_totp\s*\(/, 'production TOTP compatibility must not reintroduce a second TOTP verifier');

  requireMatch(errors, snapshot.coreLib, /max_payload_bytes:\s*usize/, 'DeserializeGuard must retain an explicit aggregate payload byte budget');
  requireMatch(errors, snapshot.coreLib, /code:\s*"VAL_PAYLOAD_TOO_LARGE"\.into\(\)/, 'DeserializeGuard must expose a stable aggregate-payload rejection code');
  requireMatch(errors, snapshot.coreLib, /pub\s+fn\s+check_raw[\s\S]*?let\s+mut\s+encoded_bytes\s*=\s*0usize[\s\S]*?self\.check_value\(payload,\s*0,\s*&mut\s+encoded_bytes\)/, 'DeserializeGuard check_raw must execute aggregate byte accounting');
  requireMatch(errors, snapshot.coreLib, /fn\s+charge_bytes[\s\S]*?encoded_bytes\s*>\s*self\.max_payload_bytes/, 'DeserializeGuard must fail closed when aggregate bytes exceed max_payload_bytes');
  requireMatch(errors, snapshot.coreLib, /fn\s+encoded_string_bytes\(/, 'DeserializeGuard must account JSON string encoding without serializing a second payload copy');
  forbidMatch(errors, snapshot.coreLib, /pub\s+fn\s+check_raw[\s\S]{0,180}?self\.check_depth\(payload,\s*0\)/, 'DeserializeGuard must not regress to depth-only checking');

  requireMatch(errors, snapshot.excelImport, /pub\s+const\s+IMPORT_FILE_BYTES_MAX:\s*usize\s*=/, 'spreadsheet import must keep an explicit single-file byte budget');
  requireMatch(errors, snapshot.excelImport, /ARCHIVE_UNCOMPRESSED_BYTES_MAX/, 'spreadsheet import must keep an archive expansion byte budget');
  requireMatch(errors, snapshot.excelImport, /ARCHIVE_EXPANSION_RATIO_MAX/, 'spreadsheet import must keep an archive expansion-ratio budget');
  requireMatch(errors, snapshot.excelImport, /fn\s+validate_zip_expansion_budget\(/, 'spreadsheet import must validate ZIP central-directory expansion before parsing');
  requireMatch(errors, snapshot.excelImport, /parse_excel_rows_from_buffer[\s\S]*?validate_import_file\(buffer,\s*file_name\)\?;/, 'every spreadsheet parse must pass the P7-B import admission gate first');
  requireMatch(errors, snapshot.excelImport, /validate_zip_expansion_budget\(buffer\)\?;/, 'ZIP-backed spreadsheet formats must pass archive expansion admission');
  requireMatch(errors, snapshot.excelImport, /IMPORT_WORKBOOK_ROWS_MAX/, 'spreadsheet import must cap workbook rows');
  requireMatch(errors, snapshot.excelImport, /IMPORT_WORKBOOK_COLUMNS_MAX/, 'spreadsheet import must cap workbook columns');
  requireMatch(errors, snapshot.excelImport, /IMPORT_WORKBOOK_CELLS_MAX/, 'spreadsheet import must cap total workbook cells');
  requireMatch(errors, snapshot.excelImport, /struct\s+TempImportFile\(/, 'spreadsheet temp-file ownership must be explicit');
  requireMatch(errors, snapshot.excelImport, /impl\s+Drop\s+for\s+TempImportFile[\s\S]*?remove_file/, 'spreadsheet temp files must be cleaned on all parser exit paths');
  forbidMatch(errors, snapshot.excelImport, /Path::new\(file_name\)[\s\S]{0,300}temp_dir\.join/, 'raw upload file names must not participate in temp paths');

  requireMatch(errors, snapshot.officialExcelImport, /REGISTRY_EXCEL_BASE64_BYTES_MAX/, 'registry Excel parser must cap base64 envelope bytes');
  requireMatch(errors, snapshot.officialExcelImport, /REGISTRY_EXCEL_FILE_BYTES_MAX/, 'registry Excel parser must cap decoded file bytes');
  requireMatch(errors, snapshot.officialExcelImport, /fn\s+registry_excel_guard\(\)[\s\S]*?max_payload_bytes:[\s\S]*?max_string_len:/, 'registry Excel commands must use an explicit bounded DeserializeGuard profile');
  const registryGuardCalls = snapshot.officialExcelImport.match(/registry_excel_guard\(\)\.check_raw\(&payload\)\?/g) ?? [];
  if (registryGuardCalls.length < 2) {
    errors.push('registry parse_excel and validate_excel must both execute the bounded Excel payload guard');
  }
  requireMatch(errors, snapshot.officialExcelImport, /fn\s+validate_excel_buffer[\s\S]*?validate_zip_expansion_budget\(buffer\)\?;/, 'registry Excel ZIP formats must pass archive expansion admission');
  requireMatch(errors, snapshot.officialExcelImport, /parse_excel_rows_from_buffer[\s\S]*?validate_excel_buffer\(buffer,\s*file_name\)\?;/, 'registry Excel parse must pass file/path/archive admission before temp-file creation');
  requireMatch(errors, snapshot.officialExcelImport, /REGISTRY_EXCEL_ROWS_MAX/, 'registry Excel parser must cap worksheet rows');
  requireMatch(errors, snapshot.officialExcelImport, /REGISTRY_EXCEL_COLUMNS_MAX/, 'registry Excel parser must cap worksheet columns');
  requireMatch(errors, snapshot.officialExcelImport, /REGISTRY_EXCEL_CELLS_MAX/, 'registry Excel parser must cap worksheet cells');
  requireMatch(errors, snapshot.officialExcelImport, /REGISTRY_DEVICE_IMPORT_ROWS_MAX/, 'registry device import must cap row-level DB/module amplification');
  requireMatch(errors, snapshot.officialExcelImport, /self\.rows\.len\(\)\s*>\s*REGISTRY_DEVICE_IMPORT_ROWS_MAX/, 'registry device import validation must reject rows above the amplification limit');
  requireMatch(errors, snapshot.officialExcelImport, /struct\s+TempExcelFile\(/, 'registry Excel temp-file ownership must be explicit');
  requireMatch(errors, snapshot.officialExcelImport, /impl\s+Drop\s+for\s+TempExcelFile[\s\S]*?remove_file/, 'registry Excel temp files must be cleaned on all parser exit paths');
  forbidMatch(errors, snapshot.officialExcelImport, /temp_dir\(\)[\s\S]{0,220}(?:file_name|sanitized_name)/, 'registry Excel temp path must not include caller-controlled file names');

  requireMatch(errors, snapshot.multipartImport, /const\s+IMPORT_MULTIPART_PARTS_MAX:\s*usize\s*=/, 'multipart import parser must keep an explicit part-count budget');
  requireMatch(errors, snapshot.multipartImport, /\.next_field\(\)\s*\.await\s*\.map_err\(/, 'multipart parser errors must be propagated instead of swallowed');
  requireMatch(errors, snapshot.multipartImport, /if\s+file\.is_some\(\)[\s\S]*?只能包含一个 file 字段/, 'multipart import parser must reject repeated file parts');
  requireMatch(errors, snapshot.multipartImport, /approved_execution\.is_some\(\)[\s\S]*?只能包含一个 approved 字段/, 'multipart import parser must reject repeated approval parts');
  requireMatch(errors, snapshot.multipartImport, /bytes\.len\(\)\s*>\s*IMPORT_FILE_BYTES_MAX/, 'multipart import parser must enforce the single-file byte budget before parsing');
  forbidMatch(errors, snapshot.orderRoute, SWALLOWED_MULTIPART, 'order import routes must not swallow multipart parser errors');
  forbidMatch(errors, snapshot.deviceRoute, SWALLOWED_MULTIPART, 'device import route must not swallow multipart parser errors');
  const orderMultipartCalls = snapshot.orderRoute.match(/multipart_import::parse_excel_import_multipart\(&mut multipart,\s*true\)\.await\?/g) ?? [];
  if (orderMultipartCalls.length < 3) {
    errors.push('all three order Excel imports must use the shared fail-closed multipart parser');
  }
  requireMatch(errors, snapshot.deviceRoute, /multipart_import::parse_excel_import_multipart\(&mut multipart,\s*false\)\.await\?/, 'device Excel import must use the shared fail-closed multipart parser');

  requireMatch(errors, snapshot.orderRoute, /const\s+ORDER_BULK_ITEMS_MAX:\s*usize\s*=\s*\d+\s*;/, 'order routes must keep an explicit bulk fanout budget');
  requireMatch(errors, snapshot.orderRoute, /const\s+ORDER_IMPORT_ROWS_MAX:\s*usize\s*=\s*[\d_]+\s*;/, 'order Excel routes must keep an execution-row budget narrower than parser admission');
  const orderImportRowGuards = snapshot.orderRoute.match(/enforce_item_limit\("导入行",\s*parsed\.rows\.len\(\),\s*ORDER_IMPORT_ROWS_MAX\)\?/g) ?? [];
  if (orderImportRowGuards.length < 3) {
    errors.push('all three order Excel imports must enforce the execution-row budget');
  }
  requireMatch(errors, snapshot.orderRoute, /ORDER_BATCH_SHIP_ORDERS_MAX/, 'batch shipment must cap order fanout');
  requireMatch(errors, snapshot.orderRoute, /ORDER_BATCH_SHIP_DEVICES_PER_ORDER_MAX/, 'batch shipment must cap per-order device fanout');
  requireMatch(errors, snapshot.orderRoute, /ORDER_BATCH_SHIP_DEVICE_REFS_MAX/, 'batch shipment must cap aggregate device references');
  requireMatch(errors, snapshot.orderRoute, /checked_add\(device_count\)/, 'batch shipment aggregate device accounting must fail closed on overflow');
  requireMatch(errors, snapshot.orderRoute, /ORDER_EXPORT_IDS_MAX/, 'order export must cap explicit ID fanout');
  requireMatch(errors, snapshot.orderRoute, /ORDER_EXPORT_ROWS_MAX/, 'order export must cap filtered result rows');
  const bulkLimitCalls = snapshot.orderRoute.match(/enforce_item_limit\("ids",[\s\S]{0,90}?ORDER_BULK_ITEMS_MAX\)\?/g) ?? [];
  if (bulkLimitCalls.length < 4) {
    errors.push('order bulk delete, notes, lookup, and update paths must enforce the shared fanout budget');
  }

  requireMatch(errors, snapshot.deviceCandidateSqlite, /pub\s+const\s+DEVICE_IMPORT_MATCH_CANDIDATES_MAX:\s*usize\s*=\s*[\d_]+\s*;/, 'device import matching must keep a finite candidate scan budget');
  requireMatch(errors, snapshot.deviceCandidateSqlite, /WHERE tenant_id = \?1[\s\S]{0,120}?LIMIT \?2/, 'SQLite device import candidates must be selected inside the scoped tenant boundary with a SQL limit');
  requireMatch(errors, snapshot.deviceCandidatePostgres, /WHERE tenant_id = \$1[\s\S]{0,120}?LIMIT \$2/, 'PostgreSQL device import candidates must be selected inside the scoped tenant boundary with a SQL limit');
  requireMatch(errors, snapshot.deviceCandidateDispatch, /DEVICE_IMPORT_MATCH_CANDIDATES_MAX\s*\+\s*1/, 'device candidate dispatch must probe max+1');
  requireMatch(errors, snapshot.deviceCandidateApp, /raw_serials\.len\(\)\s*>\s*DEVICE_IMPORT_MATCH_CANDIDATES_MAX/, 'device import candidate loading must fail closed on max+1 overflow');
  requireMatch(errors, snapshot.orderRoute, /application_services\(\)\.device_candidates\(\)/, 'order device import must use the application device-candidate facade');
  requireMatch(errors, snapshot.orderRoute, /\.list_for_import\(&ctx\)/, 'order device import must bind candidate matching to the authenticated ExecutionContext');
  forbidMatch(errors, snapshot.orderRoute, /get_device_match_candidates\s*\(|device_service::resolve_device_serial_no_for_import/, 'order device import must not return to legacy SQLite candidate matching');

  const scopedOrderListReads = snapshot.orderExportService.match(/\.execute\(\s*"order_read_compatibility",\s*"list_orders",/g) ?? [];
  if (scopedOrderListReads.length < 2) {
    errors.push('filtered order export must use scoped order-read compatibility authority for every page');
  }
  requireMatch(errors, snapshot.orderExportService, /if\s+total\s*>\s*max_rows/, 'filtered order export must reject results above its row budget');
  requireMatch(errors, snapshot.orderExportService, /\.execute\(\s*"order_read_compatibility",\s*"get_order",/, 'explicit-ID order export must resolve each order through scoped read authority');
  requireMatch(errors, snapshot.orderExportService, /if\s+ids\.len\(\)\s*>\s*max_ids/, 'explicit-ID order export must fail closed above its ID budget');
  requireMatch(errors, snapshot.orderExportService, /projection_from_value[\s\S]*?serde_json::from_value/, 'order export projection decoding must fail closed instead of silently dropping malformed rows');
  requireMatch(errors, snapshot.orderRoute, /order_export_service::load_filtered_orders/, 'order HTTP export must use the scoped bounded filtered reader');
  requireMatch(errors, snapshot.orderRoute, /order_export_service::load_orders_by_ids/, 'order HTTP export must use the scoped bounded ID reader');
  forbidMatch(errors, snapshot.orderRoute, /order_compatibility_support::query_orders(?:_by_ids)?\s*\(/, 'order HTTP export must not return to unscoped legacy order readers');

  requireMatch(errors, snapshot.deviceReadAuthority, /pub\s+const\s+DEVICE_LEGACY_LIST_ROWS_MAX:\s*usize\s*=\s*500/, 'legacy device list must keep a finite row budget');
  requireMatch(errors, snapshot.deviceReadAuthority, /pub\s+const\s+DEVICE_EXPORT_ROWS_MAX:\s*usize\s*=\s*500/, 'device export must keep a finite result budget');
  requireMatch(errors, snapshot.deviceReadAuthority, /pub\s+const\s+DEVICE_WARNING_SCAN_ROWS_MAX:\s*usize\s*=\s*5_000/, 'warning-status pagination must keep a finite pre-scan budget');
  const legacyDeviceListAuthority = rustFunctionSource(snapshot.deviceReadAuthority, /pub\s+fn\s+legacy_unpaged\s*\(/);
  requireMatch(errors, legacyDeviceListAuthority, /self\.repository_provider\.bind\(ctx\)\?/, 'legacy device list must bind the active execution context through the scoped repository authority');
  requireMatch(errors, legacyDeviceListAuthority, /scoped\.devices\(\)\.list\(&DeviceListRequest/, 'legacy device list must read through the scoped device repository');
  requireMatch(errors, legacyDeviceListAuthority, /devices\.len\(\)\s*>\s*DEVICE_LEGACY_LIST_ROWS_MAX/, 'legacy device list must fail closed above its row budget');
  requireMatch(errors, snapshot.deviceReadAuthority, /compatibility_count\(&request\)/, 'device scan/export admission must count through the scoped repository authority');
  requireMatch(errors, snapshot.deviceReadAuthority, /base_count[\s\S]{0,160}DEVICE_EXPORT_ROWS_MAX/, 'filtered device export must reject base matches above the export budget before the bounded read');
  requireMatch(errors, snapshot.deviceReadAuthority, /base_count[\s\S]{0,220}DEVICE_WARNING_SCAN_ROWS_MAX/, 'warning-status pagination must execute the scan admission budget');
  requireMatch(errors, snapshot.deviceRoute, /application_services\(\)[\s\S]{0,120}?\.device_reads\(\)[\s\S]{0,220}?\.legacy_unpaged\(/, 'unpaginated device list must use the backend-neutral scoped read authority');
  forbidMatch(errors, snapshot.deviceRoute, /device_bounded_read::list_legacy_devices_bounded/, 'unpaginated device list must not restore the SQLite-only bounded compatibility helper');
  requireMatch(errors, snapshot.deviceRoute, /application_services\(\)[\s\S]{0,120}?\.device_reads\(\)[\s\S]{0,180}?\.paged\(/, 'warning-status pagination must execute through the bounded device-read authority');
  requireMatch(errors, snapshot.deviceRoute, /\.export_filtered\(/, 'filtered device export must use the bounded device-read authority');
  forbidMatch(errors, snapshot.deviceRoute, /registry\s*\.execute\("device",\s*"list_devices"/, 'HTTP device list must not return to the unbounded legacy Registry list command');
  forbidMatch(errors, snapshot.deviceRoute, /device_service::query_devices\(&state\.pool/, 'HTTP device export must not directly invoke the unbounded compatibility query');

  requireMatch(errors, snapshot.deviceRoute, /const\s+DEVICE_BULK_ITEMS_MAX:\s*usize\s*=\s*\d+\s*;/, 'device routes must keep an explicit bulk fanout budget');
  const deviceBulkGuards = snapshot.deviceRoute.match(/raw\.len\(\)\s*>\s*DEVICE_BULK_ITEMS_MAX/g) ?? [];
  if (deviceBulkGuards.length < 2) {
    errors.push('device bulk delete and update must reject fanout above DEVICE_BULK_ITEMS_MAX');
  }
  requireMatch(errors, snapshot.deviceRoute, /sns\.len\(\)\s*>\s*DEVICE_BULK_ITEMS_MAX/, 'device explicit-ID export must enforce the bulk fanout budget');

  requireMatch(errors, snapshot.backupRoute, /const\s+BACKUP_SCAN_ENTRY_LIMIT:\s*usize\s*=\s*\d+\s*;/, 'backup status must keep a finite directory-entry scan budget');
  requireMatch(errors, snapshot.backupRoute, /enumerate\(\)[\s\S]*?index\s*>=\s*BACKUP_SCAN_ENTRY_LIMIT/, 'backup status must stop directory iteration at the scan budget');
  forbidMatch(errors, snapshot.backupRoute, /read_dir\(backup_dir\)[\s\S]{0,220}\.collect(?:\s*::<[^;()]+>)?\s*\(/, 'backup status must not collect an unbounded directory into memory');
  forbidMatch(errors, snapshot.backupRoute, /read_dir\(backup_dir\)\s*\.unwrap\s*\(/, 'backup status must not panic on directory scan failure');

  for (const [label, audit] of [
    ['backend audit', snapshot.auditService],
    ['feature audit', snapshot.featureAudit],
  ]) {
    requireMatch(errors, audit, /REDACTED_SECRET/, `${label} must retain a credential redaction marker`);
    requireMatch(errors, audit, /REDACTED_PII/, `${label} must retain restricted-PII redaction`);
    requireMatch(errors, audit, /fn\s+redact_audit_value\(/, `${label} must recursively sanitize audit detail before persistence`);
    requireMatch(errors, audit, /fn\s+is_opaque_secret_reference_key\(/, `${label} must distinguish opaque secret/credential references from secret material`);
    requireMatch(errors, audit, /let\s+opaque_reference\s*=\s*is_opaque_secret_reference_key\(&key\)\s*;/, `${label} must apply the opaque-reference classifier to normalized audit keys`);
    requireMatch(errors, audit, /let\s+secret_material\s*=\s*key\.contains\("secret"\)\s*&&\s*!opaque_reference\s*;/, `${label} must redact secret material without hiding opaque secret references`);
    requireMatch(errors, audit, /let\s+credential_material\s*=\s*key\.contains\("credential"\)\s*&&\s*!opaque_reference\s*;/, `${label} must redact credential values/material without hiding opaque credential references`);
    requireMatch(errors, audit, /redactedSha256/, `${label} must retain digest-only evidence for oversized redacted detail`);
    forbidMatch(errors, audit, /"preview"\s*:/, `${label} must not persist a plaintext preview of oversized audit detail`);
  }
  requireMatch(errors, snapshot.registrySource, /AuditPayloadPolicy::ReferenceOnly/, 'Registry execution audit must stay reference-only instead of persisting command payload/result');
  requireMatch(errors, snapshot.errorSource, /fn\s+internal_log_class\(/, 'internal AppError logging must use stable low-cardinality classification');
  requireMatch(errors, snapshot.errorSource, /error_class\s*=\s*self\.internal_log_class\(\)/, 'internal AppError tracing must log only the classified error kind');
  forbidMatch(errors, snapshot.errorSource, /tracing::error!\(\s*"Internal error:\s*\{\}"/, 'internal AppError tracing must not log raw Display details');
  requireMatch(errors, snapshot.tenantPreview, /tracing::error!\(code\s*=\s*%payload\.code,\s*"tenant preview command failed"\)/, 'tenant preview internal logging must retain code-only error evidence');
  forbidMatch(errors, snapshot.tenantPreview, /tracing::error!\([^\n]*message\s*=\s*%payload\.message/, 'tenant preview must not log raw module error messages');
  requireMatch(errors, snapshot.tenantPreview, /fn\s+unknown_preview_error_discards_module_message\(/, 'tenant preview must retain a negative regression test for secret-bearing module errors');

  requireMatch(errors, snapshot.r4Migrations, /MIGRATION_069_ID/, 'R4 migration registry must define migration 069');
  requireMatch(errors, snapshot.r4Migrations, /run_sqlite_extension_069/, 'R4 migration registry must expose SQLite 069');
  requireMatch(errors, snapshot.r4Migrations, /run_pg_extension_069/, 'R4 migration registry must expose PostgreSQL 069');
  requireMatch(errors, snapshot.r4Migrations, /MIGRATION_070_ID/, 'R4 migration registry must define migration 070');
  requireMatch(errors, snapshot.r4Migrations, /run_sqlite_extension_070/, 'R4 migration registry must expose SQLite 070');
  requireMatch(errors, snapshot.r4Migrations, /run_pg_extension_070/, 'R4 migration registry must expose PostgreSQL 070');
  requireMatch(errors, snapshot.dbMod, /run_sqlite_extension_068\(conn\)[\s\S]*run_sqlite_extension_069\(conn\)[\s\S]*run_sqlite_extension_070\(conn\)/, 'SQLite migration composition must apply 069 then 070 after 068');
  requireMatch(errors, snapshot.dbMod, /run_pg_extension_068\(pool\)[\s\S]*run_pg_extension_069\(pool\)[\s\S]*run_pg_extension_070\(pool\)/, 'PostgreSQL migration composition must apply 069 then 070 after 068');

  for (const [label, migration] of [
    ['SQLite 069', snapshot.sqliteMigration],
    ['PostgreSQL 069', snapshot.pgMigration],
  ]) {
    const sql = executableSql(migration);
    requireMatch(errors, sql, /auth_rate_limit_state/, `${label} must create auth_rate_limit_state`);
    requireMatch(errors, sql, /key_hash\s+(?:TEXT|VARCHAR\([^)]*\))\s+PRIMARY\s+KEY/i, `${label} must key rate state by digest`);
    forbidMatch(errors, sql, /\b(?:ip_address|client_ip|username)\b/i, `${label} must not persist raw IP or username rate keys`);
  }
  for (const [label, migration] of [
    ['SQLite 070', snapshot.sqlitePasswordRevocation],
    ['PostgreSQL 070', snapshot.pgPasswordRevocation],
  ]) {
    requireMatch(errors, migration, /password_hash/, `${label} must bind revocation to password-hash changes`);
    requireMatch(errors, migration, /DELETE\s+FROM\s+auth_sessions/i, `${label} must revoke every prior identity session`);
  }

  return errors;
}

export function run(root = ROOT) {
  const errors = validateSnapshot(collectSnapshot(root));
  if (errors.length > 0) {
    console.error('R4-P7 platform security hardening boundary FAILED');
    for (const error of errors) console.error(`- ${error}`);
    process.exitCode = 1;
    return false;
  }
  console.log('R4-P7 platform security hardening boundary PASS');
  return true;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  run();
}
