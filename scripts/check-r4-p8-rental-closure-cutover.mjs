#!/usr/bin/env node
import fs from 'node:fs'; import path from 'node:path'; import {fileURLToPath} from 'node:url';
const ROOT=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const P={mod:'backend/src/repositories/mod.rs',provider:'backend/src/repositories/contracts/provider.rs',dispatch:'backend/src/repositories/rental_closure_dispatch.rs',writer:'backend/src/repositories/rental_closure_postgres.rs',pgmod:'backend/src/repositories/postgres/mod.rs',live:'backend/src/repositories/postgres/rental_closure_qualification_tests.rs',main:'backend/src/main.rs'};
const read=p=>fs.readFileSync(path.join(ROOT,p),'utf8');
export const collectP8RentalClosureSnapshot=()=>Object.fromEntries(Object.entries(P).map(([k,p])=>[k,read(p)]));
export function validateP8RentalClosureSnapshot(s){const e=[];
for(const t of ['mod rental_closure_dispatch;','mod rental_closure_postgres;','pub use rental_closure_dispatch::ScopedRentalClosureRepository;']) if(!s.mod.includes(t)) e.push('Rental Closure module wiring missing: '+t);
if(!s.provider.includes('rental_closure_dispatch::ScopedRentalClosureRepository')) e.push('Rental Closure provider must use backend-neutral dispatch');
const routes=(s.dispatch.match(/PostgresRentalClosureRepository::new\(self\.scoped\.session\(\)\)/g)||[]).length; if(routes<7)e.push('Rental Closure dispatch must route all repository entrypoints');
for(const t of ['pg_write_serializable_repository','ReturnReceived','InspectionComplete','RiskCasesResolved','SettlementComplete','receiveIdentity is already bound','illegal inspection transition','DAMAGE_REVIEW_OPEN','FINANCIAL_AUTHORITY_NOT_AVAILABLE']) if(!s.writer.includes(t)) e.push('Rental Closure PostgreSQL invariant missing: '+t);
if(/\brusqlite\b|SqliteRepositorySession|SqliteConnectionManager/.test(s.writer))e.push('Rental Closure PostgreSQL writer must not depend on SQLite runtime types');
if(!s.pgmod.includes('mod rental_closure_qualification_tests;'))e.push('Rental Closure live qualification wiring missing');
for(const t of ['live_pg18_rental_closure_preserves_return_inspection_settlement_and_scope','REPOSITORY_PREVIEW_WRITE_DENIED','DAMAGE_REVIEW_OPEN','FINANCIAL_AUTHORITY_NOT_AVAILABLE','SettlementComplete']) if(!s.live.includes(t))e.push('Rental Closure live proof missing: '+t);
if(!s.main.includes('if config.is_production && pg_pool.is_none()'))e.push('Rental Closure production cutover must retain the fail-closed PostgreSQL authority guard');
if(s.main.includes('R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback'))e.push('Rental Closure cutover must not restore the retired transitional production barrier');
return e;}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){const e=validateP8RentalClosureSnapshot(collectP8RentalClosureSnapshot());if(e.length){console.error(e.join('\n'));process.exit(1)}console.log('R4-P8 Rental Closure cutover gate passed.')}
