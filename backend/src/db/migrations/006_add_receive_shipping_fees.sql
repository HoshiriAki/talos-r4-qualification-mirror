ALTER TABLE pricing_configs
ADD COLUMN receiveShippingFeesJson TEXT NOT NULL DEFAULT '{"area1":7,"area4":18,"area2":7,"area3":7}';
