<!--
generated_from:
  canonical_version: "1.0.0"
  profile: "developer"
  generated_at: "2026-08-03T13:12:00+08:00"
  canonical_ref: ".talos/knowledge/talos/"
-->

# Talos 模块套件初始化

> 给 AI：这是 Talos 模块开发套件的入口。必须从一个**已核验 implementation commit**复制权威 Core crate；不得从 Markdown 重新手写 `system-core`。
>
> 规范来源：TALOS Canonical 与 Current Rust Runtime Profile。实现来源：同一核验 commit 下的 `backend/system/core` 与 `backend/system/core-derive`。
>
> **SP-05 disposition:** `orchestration=EXPERIMENTAL_FEATURE; transport=KEEP_CORE_CANONICAL`。

## 1. 前置条件

开始前记录：

```text
implementation_repository = HoshiriAki/TESSERACT-WAREHOUSiNG
implementation_branch = prototype
implementation_commit = <verified commit>
knowledge_commit = <verified talos-knowledge commit>
```

要求：

- implementation commit 必须存在且已通过 Repository Quality；
- 读取该 commit 对应的 Current Rust Runtime Profile；
- 不从历史文档、聊天记录或旧分发副本恢复 Core 定义；
- 不把未合并 PR head 当作稳定分发基线。

## 2. 创建工作区

目标结构：

```text
./
├─ Cargo.toml
├─ AUTHORS.toml
├─ system-core/              # 从 backend/system/core 原样复制
├─ system-core-derive/       # 从 backend/system/core-derive 原样复制
├─ features/
└─ adapters/
```

根 `Cargo.toml`：

```toml
[workspace]
members = [
  "system-core",
  "system-core-derive",
  "features/*",
]
resolver = "3"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
schemars = "1"
async-trait = "0.1"
secrecy = "0.10"
zeroize = "1"
scrypt = "0.11"
aes-gcm = "0.10"
system-core = { path = "system-core" }
system-core-derive = { path = "system-core-derive" }
```

## 3. 复制权威 Core crate

从步骤 1 记录的同一 implementation commit 复制：

```text
backend/system/core        → system-core
backend/system/core-derive → system-core-derive
```

PowerShell 示例：

```powershell
$TalosSource = "<verified TESSERACT checkout>"
Copy-Item -Recurse -Force "$TalosSource/backend/system/core" "./system-core"
Copy-Item -Recurse -Force "$TalosSource/backend/system/core-derive" "./system-core-derive"
```

POSIX shell 示例：

```sh
TALOS_SOURCE="<verified TESSERACT checkout>"
cp -R "$TALOS_SOURCE/backend/system/core" ./system-core
cp -R "$TALOS_SOURCE/backend/system/core-derive" ./system-core-derive
```

禁止：

- 从本文件复制 `pub struct`、`pub enum` 或 `pub trait` 定义；
- 只复制 `lib.rs` 而遗漏子模块、features、tests 或 proc-macro crate；
- 混用不同 implementation commit 的 `system-core` 与 `system-core-derive`；
- 用未固定版本的第三方 Core 副本替换当前实现。

## 4. SP-05 Core 合约核验

复制后的 `system-core/Cargo.toml` 必须包含：

```toml
[features]
default = []
experimental-orchestration = []
```

默认 Core 必须继续在 crate root 暴露当前 Profile 定义的 Transport group：

```text
DataTransport
TransportMetadata
TransportPriority
TransportOptions
TransportMessage
TransportHealth
```

编排数据类型不得在默认 crate root 暴露。受控实验必须显式启用：

```toml
[dependencies]
system-core = { workspace = true, features = ["experimental-orchestration"] }
```

并从以下路径导入：

```rust
use system_core::experimental::{
    CompensationLog,
    ModuleOp,
    OrchestrationError,
    SagaStep,
};
```

这些类型只描述内存数据，不提供执行器、持久化、outbox/inbox、重放、恢复、幂等、崩溃安全补偿或 `UnknownOutcome` 语义。

## 5. 编译核验

```sh
cargo check -p system-core --no-default-features
cargo test -p system-core --no-default-features
cargo check -p system-core --no-default-features --features experimental-orchestration
cargo test -p system-core --no-default-features --features experimental-orchestration
cargo check --workspace
cargo test --workspace
```

任一命令失败时停止，不得通过复制旧定义或增加 broad suppression 绕过。

## 6. 创建业务模块

业务模块使用 [MODULE_TEMPLATE.md](./MODULE_TEMPLATE.md)。默认模块不得启用实验性编排 feature。只有具备独立 Story/ADR、明确 owner 和删除/替换条件的受控实验才能启用。

模块依赖示例：

```toml
[dependencies]
system-core.workspace = true
system-core-derive.workspace = true
serde.workspace = true
serde_json.workspace = true
schemars.workspace = true
```

## 7. 分发投影规则

- Canonical 定义规范，Profile 记录当前实现事实，implementation source 提供可编译 crate；
- module-kit 只描述复制、启用与验证流程，不再复制完整 Core 实现；
- `DataTransport` 的终态所有权仍由未来 Integration Runtime / Provider Connector ADR 决定；当前保持 `KEEP_CORE_CANONICAL`；
- orchestration primitives 保持 `EXPERIMENTAL_FEATURE`，不得被文档描述为可运行 Saga；
- 任何 disposition 变化必须同步 Canonical/Profile、implementation、module-kit、Structural Gate 与迁移证据。
