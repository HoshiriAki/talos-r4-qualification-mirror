#!/usr/bin/env node
import assert from 'node:assert/strict'; import {collectP8RentalClosureSnapshot,validateP8RentalClosureSnapshot} from './check-r4-p8-rental-closure-cutover.mjs';
function fail(name,fn,needle){const s=structuredClone(collectP8RentalClosureSnapshot());fn(s);const e=validateP8RentalClosureSnapshot(s);assert.ok(e.some(x=>x.includes(needle)),name+': '+JSON.stringify(e));}
assert.deepEqual(validateP8RentalClosureSnapshot(collectP8RentalClosureSnapshot()),[]);
fail('dispatch',(s)=>s.dispatch=s.dispatch.replaceAll('PostgresRentalClosureRepository::new(self.scoped.session())','REMOVED_PG_ROUTE'),'dispatch must route');
fail('serializable',(s)=>s.writer=s.writer.replaceAll('pg_write_serializable_repository','pg_write_serializable'),'pg_write_serializable_repository');
fail('outbox',(s)=>s.writer=s.writer.replaceAll('SettlementComplete','REMOVED_SETTLEMENT_EVENT'),'SettlementComplete');
fail('preview',(s)=>s.live=s.live.replaceAll('REPOSITORY_PREVIEW_WRITE_DENIED','REMOVED_PREVIEW'),'REPOSITORY_PREVIEW_WRITE_DENIED');
fail('production-guard',(s)=>s.main=s.main.replace('if config.is_production && pg_pool.is_none()','if false'),'fail-closed PostgreSQL authority guard');
fail('retired-barrier',(s)=>s.main+='\nconst RETIRED_P8_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n','must not restore the retired transitional production barrier');
console.log('R4-P8 Rental Closure cutover mutation tests passed.');
