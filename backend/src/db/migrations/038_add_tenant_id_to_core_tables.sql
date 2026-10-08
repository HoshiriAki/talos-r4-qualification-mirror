-- Migration 038: Add tenant_id to core tables for data isolation
--
-- This migration adds tenant_id column to all core business tables,
-- ensuring each tenant's data is isolated from others.

-- Orders table
ALTER TABLE orders ADD COLUMN tenant_id TEXT;
UPDATE orders SET tenant_id = 'default' WHERE tenant_id IS NULL;

-- Devices table
ALTER TABLE devices ADD COLUMN tenant_id TEXT;
UPDATE devices SET tenant_id = 'default' WHERE tenant_id IS NULL;

-- Warehouses table
ALTER TABLE warehouses ADD COLUMN tenant_id TEXT;
UPDATE warehouses SET tenant_id = 'default' WHERE tenant_id IS NULL;

-- Device models table
ALTER TABLE device_models ADD COLUMN tenant_id TEXT;
UPDATE device_models SET tenant_id = 'default' WHERE tenant_id IS NULL;

-- Audit logs table
ALTER TABLE audit_logs ADD COLUMN tenant_id TEXT;
UPDATE audit_logs SET tenant_id = 'default' WHERE tenant_id IS NULL;

-- User settings table
ALTER TABLE user_settings ADD COLUMN tenant_id TEXT;
UPDATE user_settings SET tenant_id = 'default' WHERE tenant_id IS NULL;

-- Order devices junction table
ALTER TABLE order_devices ADD COLUMN tenant_id TEXT;
UPDATE order_devices SET tenant_id = 'default' WHERE tenant_id IS NULL;
