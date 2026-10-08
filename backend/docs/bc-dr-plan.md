# BC/DR Plan — Talos 设备租赁管理系统

## 1. 概述

本文档定义了 Talos 系统的业务连续性（BC）和灾难恢复（DR）计划，符合信息安全等级保护三级（等保三级）基础要求。

| 指标 | 目标值 |
|------|--------|
| RTO (Recovery Time Objective) | < 4 小时 |
| RPO (Recovery Point Objective) | < 1 小时 |
| 最大可容忍停机时间 (MTD) | 8 小时 |
| 备份保留期 | 30 天 |

## 2. 备份策略

### 2.1 数据库备份

- **方式**: 每日全量 SQLite DB 文件备份
- **加密**: AES-256-GCM 加密后存储
- **频率**: 每日凌晨 2:00 (Asia/Shanghai, UTC+8)
- **保留**: 30 天滚动保留
- **存储位置**: 
  - 主备份: 本地磁盘（`./backups/daily/`）
  - 异地: 加密后传输至异地存储（如 S3 存储桶、SFTP 服务器）

### 2.2 备份加密

使用 AES-256-GCM 算法对备份文件进行加密：

```
Backup File → AES-256-GCM(random nonce, secret key) → Encrypted Backup
                                                    → GCM Authentication Tag
```

- 密钥管理: 环境变量 `BACKUP_ENCRYPTION_KEY` (32 字节 hex 编码)
- 每次备份使用随机 12 字节 nonce
- 认证标签附加在加密文件末尾

### 2.3 备份验证

- 每次备份后自动执行完整性校验
- 每周执行一次恢复演练（还原至临时数据库，验证数据一致性）
- 每月执行一次完整 DR 演练

## 3. 灾难场景与恢复流程

### 3.1 场景一：服务器故障

**症状**: 服务器硬件故障、操作系统崩溃、不可启动

**恢复步骤**:
1. 准备新服务器环境（操作系统、依赖项）
2. 从异地备份中获取最近的加密备份文件
3. 使用备份密钥解密备份
4. 将数据库文件部署至新服务器
5. 启动应用服务
6. 验证系统功能（健康检查、核心业务流）
7. 切换 DNS/负载均衡器到新服务器

**预计 RTO**: 3-4 小时

### 3.2 场景二：数据库损坏

**症状**: 数据查询异常、应用错误日志中出现损坏警告、SQLITE_CORRUPT 错误

**恢复步骤**:
1. 停止写入流量（维护模式）
2. 定位最近一次已验证的完整备份
3. 解密备份并还原
4. 应用 WAL 日志（如有）恢复最近 1 小时内数据
5. 验证数据完整性（行数检查、关键表校验和）
6. 关闭维护模式，恢复服务

**预计 RTO**: 1-2 小时  
**预计 RPO**: < 1 小时

### 3.3 场景三：勒索软件攻击

**症状**: 文件被加密、勒索通知、系统不可用

**恢复步骤**:
1. 立即隔离受影响系统（断开网络）
2. 通知安全团队和管理层
3. 从已知干净的备份中恢复
4. 检查备份文件是否被篡改（校验和、GCM 认证标签）
5. 在隔离环境中还原系统
6. 全面安全扫描后再接入网络
7. 事后分析和加固

**预计 RTO**: 4-6 小时

### 3.4 场景四：数据删除请求

**症状**: 用户或监管要求删除个人数据

**流程**:
1. 验证请求合法性
2. 在 `data_deletion_requests` 表中创建请求记录
3. 在 15 个工作日内完成处理
4. 匿名化用户 PII（name/email/phone → ANONYMIZED-{uuid}）
5. 保留匿名化后的业务数据用于审计
6. 记录处理和完成时间戳

## 4. 系统高可用

### 4.1 应用层

- 无状态设计: 所有会话数据存储在 SQLite 数据库中
- 读写分离: SQLite WAL 模式支持并发读
- 快速启动: Rust 编译为原生二进制，启动时间 < 3 秒

### 4.2 监控与告警

- 健康检查端点: `GET /health` 
- 数据库连接池监控: 定期检查 `pool.state()`
- 磁盘空间告警: 磁盘使用率 > 85% 告警
- 备份失败告警: 连续两次备份失败触发告警

## 5. 安全控制（等保三级基础）

### 5.1 访问控制

- 基于角色的访问控制（RBAC）: admin / staff
- Cookie 会话认证（HttpOnly, SameSite=Lax, Secure）
- 密码使用 scrypt 加密存储（16 字节随机 salt, 64 字节 hash）
- 登录速率限制: 10 次/30s, 30s 封锁

### 5.2 审计追踪

- 所有操作写入 `audit_logs` 表
- 异步缓冲写入（50 条/批次, 5 秒刷新间隔）
- 审计日志不可删除（只追加）

### 5.3 数据保护

- 传输中: HTTPS/TLS 加密
- 休眠中: AES-256-GCM 备份加密
- 删除: 支持数据删除请求和匿名化
- 隐私: 隐私同意书记录和版本管理

### 5.4 双因素认证

- TOTP (Time-based One-Time Password)
- HMAC-SHA256 算法, 30 秒时间窗口
- 管理员可强制启用/禁用
- 密钥以 hex 编码存储在 `identities.totp_secret_ciphertext` 列

## 6. 演练计划

| 频率 | 演练内容 | 参与人员 |
|------|---------|---------|
| 每周 | 备份恢复测试（本地验证） | 运维 |
| 每月 | 完整 DR 演练（异地恢复） | 运维 + 管理员 |
| 每季度 | 勒索软件场景演练 | 运维 + 安全 |

## 7. 联系人

- 系统管理员: 通过 `.env` 配置文件指定
- 安全事件响应: 24 小时通知流程

## 8. 附录

### 8.1 备份脚本示例

```bash
#!/bin/bash
# Daily backup script for Talos
DB_PATH="./data/talos.db"
BACKUP_DIR="./backups/daily"
ENCRYPTION_KEY="${BACKUP_ENCRYPTION_KEY}"

DATE=$(date -d "today" +%Y%m%d --utc)
BACKUP_FILE="${BACKUP_DIR}/talos_${DATE}.db"

# Copy SQLite DB (safe with WAL checkpoint)
sqlite3 "$DB_PATH" "PRAGMA wal_checkpoint(TRUNCATE);"
cp "$DB_PATH" "$BACKUP_FILE"

# Encrypt with AES-256-GCM
openssl enc -aes-256-gcm -K "$ENCRYPTION_KEY" -iv $(xxd -l 12 -p /dev/urandom) \
  -in "$BACKUP_FILE" -out "${BACKUP_FILE}.enc"

# Remove unencrypted backup
rm "$BACKUP_FILE"

# Keep 30 days of backups
find "$BACKUP_DIR" -name "*.enc" -mtime +30 -delete
```

### 8.2 版本历史

| 版本 | 日期 | 变更 |
|------|------|------|
| 1.0 | 2026-07-01 | 初始版本 — 等保三级基础 |
