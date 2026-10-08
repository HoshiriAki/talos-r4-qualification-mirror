<!--
generated_from:
  canonical_version: "1.0.0"
  profile: "developer"
  generated_at: "2026-07-23T00:00:00+08:00"
  canonical_ref: ".talos/knowledge/talos/"
-->

# Talos 模块开发指南

> 跟着这个文档，从零写出一个符合 Talos 标准的工业化模块。
>
> **规范来源**：本文件是 [TALOS Canonical](../.talos/knowledge/talos/) 的 developer profile 投影。
> 以下 canonical ID 定义了本指南引用的核心契约：
> - `TALOS-RT-001` ExecutionContext | `TALOS-RT-003` ExecutionMode | `TALOS-RT-004` CommandMetadata
> - `TALOS-SDK-001` SystemModule | `TALOS-SDK-003` ErrorPayload | `TALOS-SDK-004` Validation Pipeline
> - `TALOS-DT-001` DataScope | `TALOS-DT-004` StaticSql
> - `TALOS-UI-001` Stable UI Contract + Implementation Profile
> - `TALOS-SEC-001` Security Baseline
>
> 核心类型不得在多个分发文件中重复手写；以 canonical 定义为唯一事实源。

> **SP-05 disposition:** `orchestration=EXPERIMENTAL_FEATURE; transport=KEEP_CORE_CANONICAL`。
> `DataTransport` 继续由 Current Rust Runtime Profile 定义为当前根级 Core ABI。
> Orchestration primitives 是非默认实验性数据契约；必须显式启用 `experimental-orchestration`，不提供运行时、持久化、重放或崩溃安全保证，并可能由后续 ADR 替换。

## 0. 前置：设置署名

**给 AI 的指令**：在开始写模块之前，从当前工作目录开始向上查找 `AUTHORS.toml` 文件（搜索深度最多 5 层父目录）。
- **找到** → 读取文件，找到当前系统用户名（`whoami`）对应的 author key，记录备用。
- **找不到** → 向用户提问："请设置你的 Talos 签名（英文，如 hoshi）："。用户回答后，在当前工作目录创建 `AUTHORS.toml`：

```toml
[authors.<用户的回答>]
name = "<用户的回答>"
```

记录该 key 备用。完成后继续第 1 步，并在模板中所有 `{{author}}` 处替换为该 key。

---

## 1. 一句话理解

一个 Talos 模块 = **一个能独立 `cargo run` 的工具** + **一个能被系统加载的插件** + **一个 AI 可发现的函数集**。

三者共享同一份代码。写一次，三种用法。

---

## 2. 你的模块长什么样

```
features/feature_<name>/
├── Cargo.toml           # 声明你是谁，依赖谁
├── src/
│   ├── lib.rs           # 全部业务逻辑 + SystemModule trait
│   └── main.rs          # 仅初始化 → 调用 → 打印结果
```

`lib.rs` 是你真正的产品。`main.rs` 是产品的展示台。

---

## 3. 最小可工作模板

### 3.1 Cargo.toml

```toml
[package]
name = "feature-<name>"          # kebab-case
version = "0.1.0"
edition = "2024"
description = "<一句话描述这个模块做什么>"

[dependencies]
system-core.workspace = true
serde.workspace = true
serde_json.workspace = true
schemars.workspace = true
secrecy.workspace = true
zeroize.workspace = true
```

> 如果你的模块需要额外的 crate（如 `sha2`, `base64`, `rand`, `scrypt`, `aes-gcm`），在此处追加。涉及加密/签名场景时请参考 §12 安全速查。

---

### 3.2 lib.rs

```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use system_core::*;

// ── API 契约（输入类型 — 面向外部调用者）─────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Sanitize)]
#[sanitize(trim, strip_control, nfc)]
pub struct GreetInput {
    /// 要问候的名字
    #[sanitize(trim, nfc)]
    pub name: String,
    /// 可选称谓
    #[sanitize(trim)]
    pub title: Option<String>,
}

impl Validate for GreetInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.name.is_empty() {
            errors.push(FieldError {
                field: "name".into(),
                message: "名字不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if self.name.len() > 100 {
            errors.push(FieldError {
                field: "name".into(),
                message: "名字不能超过 100 个字符".into(),
                code: "VAL_MAX_LENGTH".into(),
            });
        }
        if let Some(ref title) = self.title {
            if title.len() > 20 {
                errors.push(FieldError {
                    field: "title".into(),
                    message: "称谓不能超过 20 个字符".into(),
                    code: "VAL_MAX_LENGTH".into(),
                });
            }
        }
        ValidationResult { errors }
    }
}

// ── 领域类型（内部业务逻辑 — 禁止直接暴露为 API 输入）───

#[derive(Debug, Clone)]
struct Greeting {
    display_name: String,
    timestamp: u64,
}

impl From<GreetInput> for Greeting {
    fn from(input: GreetInput) -> Self {
        let display_name = match input.title {
            Some(t) => format!("{} {}", t, input.name),
            None => input.name,
        };
        Greeting {
            display_name,
            timestamp: 0, // TODO: 真实时间戳
        }
    }
}

// ── 输出类型 ──────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GreetOutput {
    pub message: String,
    pub timestamp: u64,
}

// ── 模块主体 ──────────────────────────────

pub struct FeatureHello {
    hasher: Option<Box<dyn PasswordHasher>>,
    encryptor: Option<Box<dyn SymmetricEncryptor>>,
    signer: Option<Box<dyn Signer>>,
    keystore: Option<Box<dyn KeyStore>>,
}

impl FeatureHello {
    /// 纯业务逻辑 — 禁止在此处做参数校验（校验已在管线中完成）
    fn do_greet(&self, greeting: &Greeting, ctx: &ExecutionContext) -> GreetOutput {
        GreetOutput {
            message: format!("你好，{}！[trace={}]", greeting.display_name, ctx.trace_id),
            timestamp: greeting.timestamp,
        }
    }
}

impl SystemModule for FeatureHello {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "feature_hello".into(),
            version: "0.1.0".into(),
            description: "多语言问候模块".into(),
            author: "{{author}}".into(),
        }
    }

    fn init(&mut self, _config: Value) -> Result<(), String> {
        // 安全 trait 通过 config 注入（如果模块需要）：
        // self.hasher = config.get("password_hasher").map(...);
        // self.encryptor = config.get("encryptor").map(...);
        Ok(())
    }

    fn execute(&self, command: &str, payload: Value, ctx: &ExecutionContext) -> Result<Value, String> {
        match command {
            "greet" => {
                // ── 第 1 层：反序列化守卫（大小/深度检查）────────
                DeserializeGuard::default().check_raw(&payload)
                    .map_err(|e| serde_json::to_string(&ErrorPayload {
                        category: "val".into(),
                        code: "VAL_DESERIALIZE".into(),
                        message: e.to_string(),
                        field: None,
                        context: None,
                    }).unwrap())?;

                // ── 第 2 层：反序列化 ──────────────────────────
                let input: GreetInput = serde_json::from_value(payload)
                    .map_err(|e| serde_json::to_string(&ErrorPayload {
                        category: "val".into(),
                        code: "VAL_DESERIALIZE".into(),
                        message: format!("参数解析失败: {e}"),
                        field: None,
                        context: None,
                    }).unwrap())?;

                // ── 第 3 层：类型状态管线 — 清理 + 校验 ────────
                // Unvalidated → Sanitized → Validated
                // 跳过任何一层都会导致编译失败
                let validated = Unvalidated::new(input)
                    .sanitize()
                    .validate()
                    .map_err(|e| e.to_json())?;

                // ── 第 4 层：Input → Domain → 业务逻辑 ────────
                let greeting = Greeting::from(validated.into_inner());
                let output = self.do_greet(&greeting, ctx);

                serde_json::to_value(output)
                    .map_err(|e| serde_json::to_string(&ErrorPayload {
                        category: "sys".into(),
                        code: "SYS_SERIALIZE".into(),
                        message: format!("序列化失败: {e}"),
                        field: None,
                        context: None,
                    }).unwrap())
            }
            _ => Err(serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_UNKNOWN_COMMAND".into(),
                message: format!("未知命令: {command}"),
                field: None,
                context: None,
            }).unwrap()),
        }
    }

    fn shutdown(&mut self) -> Result<(), String> {
        // 模块可在此处清理资源（关闭连接、刷新缓冲区、撤销令牌等）
        Ok(())
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "feature_hello".into(),
            description: "多语言问候模块".into(),
            commands: vec![CommandSchema {
                name: "greet".into(),
                description: "生成一条问候语".into(),
                version: "1.0.0".into(),
                input_schema: serde_json::to_value(schemars::schema_for!(GreetInput)).ok(),
                output_schema: serde_json::to_value(schemars::schema_for!(GreetOutput)).ok(),
            }],
        }
    }
}
```

> **规则**：
> - 必须使用类型状态管线 `Unvalidated → Sanitized → Validated` — Rust 编译器在编译期强制完整执行，跳过任何一层都会导致编译失败（§12.18.5）。
> - API 输入类型（`GreetInput`）与领域类型（`Greeting`）必须分离，中间经过 `From` / `TryFrom` 映射（§12.18.4）。
> - `Err(String)` 必须是 JSON 序列化的 `ErrorPayload`，禁止纯文本字符串（§12.19.2）。
> - 第 3 层的 `validate()` 和第 4 层的业务逻辑**禁止混在同一个 match 分支中** — 业务逻辑必须委托给独立的 `do_*` 方法（§12.18.1）。

---

### 3.3 main.rs

```rust
fn main() {
    let mut module = feature_hello::FeatureHello {
        hasher: None,
        encryptor: None,
        signer: None,
        keystore: None,
    };
    system_core::SystemModule::init(&mut module, serde_json::json!({}))
        .expect("模块初始化失败");

    // ── 自我介绍 ──
    let meta = system_core::SystemModule::metadata(&module);
    println!("=== {} v{} ===", meta.name, meta.version);
    println!("  {}\n", meta.description);

    // ── 冒烟测试：对每个 command 执行一次 ──
    let ctx = ExecutionContext::new_test();
    let mut passed = 0u32;
    let mut failed = 0u32;

    let commands = vec![
        ("greet", serde_json::json!({"name": "Penrose"})),
    ];

    for (cmd, payload) in commands {
        println!("[Command: {cmd}]");
        print!("  Input : ");
        println!("{}", serde_json::to_string(&payload).unwrap());

        match system_core::SystemModule::execute(&module, cmd, payload, &ctx) {
            Ok(result) => {
                print!("  Output: ");
                println!("{}", serde_json::to_string_pretty(&result).unwrap());
                println!("  Status: OK\n");
                passed += 1;
            }
            Err(e) => {
                println!("  Status: FAILED");
                println!("  Error : {e}\n");
                failed += 1;
            }
        }
    }

    // ── 确定性验证 ──
    // 简单模块至少检查 execute 返回 Ok；复杂模块应检查输出字段
    assert!(passed > 0, "所有命令测试均失败");

    // ── AI Schema ──
    let schema = system_core::to_openai_function_schema(&module);
    println!("[AI Schema]");
    println!("{}", serde_json::to_string_pretty(&schema).unwrap());

    println!("\n✓ 所有命令测试完成：{passed} 通过, {failed} 失败");

    // §12.27.5：main.rs 必须在失败时以退出码 1 退出
    if failed > 0 {
        std::process::exit(1);
    }
}
```

> **规则**：
> - `main.rs` 不写任何业务逻辑。只负责初始化 → 调用 → 断言 → 打印。
> - 必须使用 `assert!()` 进行确定性验证，禁止仅打印结果而不判断对错（§12.27.5）。
> - 失败时必须以退出码 1 退出。

---

## 4. 跑起来

```bash
# 独立运行
cargo run -p feature-<name>

# 全仓编译（确认没破坏其他模块）
cargo build --workspace
```

你应该看到类似这样的输出：

```
=== feature_hello v0.1.0 ===
  多语言问候模块

[Command: greet]
  Input : {"name":"Penrose"}
  Output: {
    "message": "你好，Penrose！",
    "timestamp": 0
  }
  Status: OK

[AI Schema]
{
  "module": "feature_hello",
  "functions": [...]
}

✓ 所有命令测试完成
```

---

## 5. 注册进系统

让系统能加载你的模块：

### 5.1 加入模块清单

在 `docs/ARCHITECTURE.md` 的模块清单表新增一行：

```markdown
| `feature_<name>` | `command1`, `command2` | <描述> | ✅/❌ |
```

### 5.2 注册 Wasm（如果是 wasm_compatible）

在 `adapters/wasm-frontend/src/lib.rs` 的 `Runtime::new()` 中加入：

```rust
modules.push(Box::new(feature_<name>::Feature<Name>));
```

### 5.3 加入 workspace 成员

确认 `Cargo.toml` 的 `[workspace.members]` 中包含：

```toml
"features/feature_<name>"
```

### 5.4 标记作者

`metadata().author` 已在第 0 步自动完成。系统加载模块时对照 `AUTHORS.toml`：匹配到 → 显示为原生模块 + 作者名；未匹配 → 显示为第三方模块。

---

## 6. 你有三种模块类型可选

| 类型 | trait | 适用场景 | 额外依赖 |
|------|-------|---------|---------|
| **SystemModule** | `SystemModule` | 业务逻辑、工具、数据处理 | 无 |
| **DataStore** | `DataStore` | 数据库连接 | `tokio-postgres` 或 `rusqlite`，`async-trait` |
| **DataTransport** | `DataTransport` | 网络通信 | 按协议而定 |

大多数情况下你只需要 **SystemModule**。

---

## 7. 命令命名规则

```
动词_宾语

parse_cookies        # 解析
generate_sign        # 生成
compose_message      # 合成
validate_input       # 校验
query_by_id          # 查询
```

`snake_case`，见名知义。每个命令必须在 `schema()` 中有对应的 `CommandSchema`。

---

## 8. 错误处理铁律

### 8.1 结构化错误（ErrorPayload）

所有 `execute()` 返回的 `Err(String)` **必须**是 JSON 序列化的 `ErrorPayload`，禁止纯文本字符串：

```rust
// ✓ 正确 — 结构化错误
Err(serde_json::to_string(&ErrorPayload {
    category: "rgv".into(),        // 错误类别：val / sys / rgv / biz
    code: "RGV_001".into(),        // 错误码：类别缩写 + 数字
    message: "风控拦截：检测到异常输入模式".into(),
    field: Some("name".into()),    // 关联字段（可选）
    context: None,                  // 附加上下文（可选）
}).unwrap())

// ✗ 禁止 — 纯文本错误
Err(format!("参数解析失败: {e}"))
Err("RGV587_ERROR: 风控触发".to_string())
```

### 8.2 错误类别速查

| category | 含义 | code 前缀 | 示例 |
|----------|------|-----------|------|
| `val` | 校验错误 | `VAL_` | `VAL_REQUIRED`, `VAL_MAX_LENGTH`, `VAL_DESERIALIZE` |
| `sys` | 系统错误 | `SYS_` | `SYS_UNKNOWN_COMMAND`, `SYS_SERIALIZE`, `SYS_INTERNAL` |
| `rgv` | 风控/安全拦截 | `RGV_` | `RGV_001`, `RGV_SQL_INJECTION`, `RGV_RATE_LIMIT` |
| `biz` | 业务逻辑错误 | `BIZ_` | `BIZ_NOT_FOUND`, `BIZ_DUPLICATE`, `BIZ_INSUFFICIENT` |

> **注意**：旧的魔术前缀 `RGV587_ERROR` 和 `RISK_CONTROL_TRIGGERED` 已废弃（§12.19.2）。所有错误必须使用结构化 `ErrorPayload`。

### 8.3 panic 禁止

```rust
// ✗ 绝对禁止
.unwrap()           // 会崩
.expect("...")      // 会崩（仅在 main.rs 的 setup 阶段允许）
panic!("...")       // 会崩
```

业务逻辑中所有可能失败的调用都必须返回 `Result`。

---

## 9. Wasm 兼容性自检

问自己一个问题：我的模块依赖了下面这些吗？

| 依赖 | 如果有 | 结论 |
|------|--------|------|
| `tokio`, `rusqlite`, `tokio-postgres` | 有 | `wasm_compatible = false` |
| 以上都没有 | ✅ | `wasm_compatible = true` → 需要注册进 Wasm |

仅依赖 `serde` / `serde_json` / `sha2` / `base64` / `rand` 这类纯算法的模块天然 wasm 兼容。

---

## 10. 提交前检查清单

### 10.1 结构完整性

```
[ ] lib.rs 实现了 SystemModule trait（含 shutdown()）
[ ] main.rs 存在且可独立运行（cargo run -p feature-xxx）
[ ] main.rs 不含业务逻辑，lib.rs 不含 main 函数
[ ] main.rs 使用 assert!() 进行确定性验证，失败时 exit(1)
[ ] author 字段已填写，与 AUTHORS.toml 注册名一致
```

### 10.2 类型与管线

```
[ ] 每个 command 有对应的输入输出 struct + JsonSchema derive
[ ] 输入类型实现了 Sanitize（#[sanitize(trim, strip_control, nfc)] 或手动 impl）
[ ] 输入类型实现了 Validate → ValidationResult（聚合所有字段错误）
[ ] API 输入类型与领域类型分离（Input → From/TryFrom → Domain）
[ ] execute() 使用类型状态管线 Unvalidated → Sanitized → Validated
[ ] execute() 签名包含 ctx: &ExecutionContext 参数
[ ] 第 3 层（validate）和第 4 层（业务逻辑）未混在同一 match 分支
```

### 10.3 错误与安全

```
[ ] 所有 Err(String) 均为 JSON 序列化的 ErrorPayload（禁止纯文本）
[ ] 未使用废弃的 RGV587_ERROR / RISK_CONTROL_TRIGGERED 魔术前缀
[ ] 如果模块涉及密码 → 注入 PasswordHasher trait，禁止 DIY 加密
[ ] 如果模块涉及加密 → 注入 Encryptor / Signer trait
[ ] 如果模块涉及密钥 → 通过 KeyStore 访问，禁止硬编码密钥
[ ] 如果模块涉及 SQL → 使用 StaticSql，禁止字符串拼接 SQL
[ ] 密钥材料使用 Secret<T> 包装（禁止打印/Debug/日志输出）
```

### 10.4 编译与测试

```
[ ] cargo build --workspace 通过
[ ] cargo test -p feature-<name> 通过（含单元测试 + 集成测试）
[ ] cargo run -p feature-<name> 打印了所有 command 的测试结果且 exit=0
[ ] ARCHITECTURE.md 模块清单新增一行
[ ] README.md 存在，包含所有公共命令的描述、示例和错误码
[ ] 如果 wasm_compatible，已在 wasm-frontend 注册
```

---

## 11. 模块工业化自评

| 标准 | 含义 | 我满足了吗 |
|------|------|-----------|
| 自包含 | `cargo run` 能直接跑，脱离系统可用 | |
| 自描述 | `metadata()` 完整，`schema()` 齐全，每个 command 有 version | |
| 自验证 | `main.rs` 对所有 command 冒烟，`assert!()` 确定性验证 | |
| 可发现 | AI Schema 导出正常 | |
| 可组合 | 不 import 其他 feature，仅依赖 trait | |
| 可替换 | 同 trait 实现者可互换（如果适用） | |
| 有版本 | Cargo.toml version 与 metadata 一致，CommandSchema.version 已填 | |
| 安全注入 | 安全 trait（PasswordHasher/Encryptor/Signer/KeyStore）通过 init 注入，禁止 DIY | |
| 管线完整 | 4 层管线（DeserializeGuard → Sanitize → Validate → Execute）完整执行 | |
| 生命周期 | shutdown() 已实现，资源清理逻辑正确 | |
| 测试覆盖 | 单元测试 + 集成测试通过，`cargo test` 无失败 | |
| 文档完备 | README.md 包含命令描述、使用示例、错误码表 | |

---

## 12. 安全速查

模块涉及安全敏感操作时，必须使用系统注入的 trait，**严禁 DIY 加密实现**。

### 12.1 密码处理

```rust
// 注入 PasswordHasher → 使用 trait 方法
let hash = self.hasher
    .as_ref()
    .ok_or(to_error("SYS_NO_HASHER", "未注入 PasswordHasher"))?
    .hash(password)
    .map_err(|e| to_error("SYS_HASH_FAILED", &e.to_string()))?;

// 验证
let valid = self.hasher.as_ref().unwrap().verify(password, &stored_hash)?;
```

- 必须使用 `scrypt`（默认）或 `argon2id`，禁止 `bcrypt` / `sha256` / `md5`
- 密码原文用 `Secret<String>` 包装，禁止在日志/Debug 输出中出现

### 12.2 加密 / 解密

```rust
// SymmetricEncryptor（AES-256-GCM-SIV + AAD）
let ciphertext = self.encryptor
    .as_ref()
    .ok_or(to_error("SYS_NO_ENCRYPTOR", "未注入 Encryptor"))?
    .encrypt(plaintext, aad)
    .map_err(|e| to_error("SYS_ENCRYPT_FAILED", &e.to_string()))?;

// ML-KEM 密钥封装（后量子）
let (ciphertext, shared_secret) = self.encryptor.as_ref().unwrap()
    .encapsulate(&public_key)?;
```

- 对称：AES-256-GCM-SIV，必须带 AAD（关联认证数据）
- 非对称：ML-KEM-1024（长期）或 ML-KEM-768（会话）
- 禁止：AES-ECB、RSA PKCS#1v1.5、裸 CBC、自创算法

### 12.3 签名 / 验签

```rust
let signature = self.signer
    .as_ref()
    .ok_or(to_error("SYS_NO_SIGNER", "未注入 Signer"))?
    .sign(message)
    .map_err(|e| to_error("SYS_SIGN_FAILED", &e.to_string()))?;

let valid = self.signer.as_ref().unwrap().verify(message, &signature)?;
```

- ML-DSA-87（默认）或 ML-DSA-65
- 禁止：RSA-PKCS#1v1.5 签名、ECDSA with non-deterministic nonce

### 12.4 密钥管理

```rust
// 禁止硬编码密钥 — 必须通过 KeyStore
let key = self.keystore
    .as_ref()
    .ok_or(to_error("SYS_NO_KEYSTORE", "未注入 KeyStore"))?
    .get_key("master_key_v1")
    .map_err(|e| to_error("SYS_KEY_NOT_FOUND", &e.to_string()))?;
```

- 密钥永远不写死在代码或配置文件中
- 支持密钥轮换（`rotate_key`）和吊销（`revoke_key`）
- 密钥句柄本身用 `Secret<KeyHandle>` 包装

### 12.5 SQL 注入防护

```rust
// ✓ 正确 — StaticSql 编译期强制，禁止运行时字符串拼接
use system_core::static_sql;
let query = static_sql!("SELECT id, name FROM users WHERE status = $1");

// ✗ 禁止 — 任何形式的字符串拼接
let query = format!("SELECT * FROM users WHERE id = {}", user_input);
```

`StaticSql` 没有 `From<&str>` 实现 — 用 `&str` 传参会导致编译错误。

### 12.6 密钥材料安全

```rust
use secrecy::{Secret, ExposeSecret};

// ✓ 密钥用 Secret<T> 包装
let api_key: Secret<String> = Secret::new("sk-xxx".into());

// ✓ 仅在需要时 expose
let raw: &str = api_key.expose_secret();

// ✗ 禁止打印/Debug/日志
println!("{:?}", api_key);       // Secret 不实现 Debug
log::info!("key: {}", api_key);  // Secret 不实现 Display
```

---

## 13. 模块测试

### 13.1 测试结构

```
features/feature_<name>/
├── src/
│   ├── lib.rs
│   └── main.rs
└── tests/
    └── integration.rs      # 集成测试（通过 execute() 调用）
```

### 13.2 单元测试（lib.rs 底部）

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_name_required() {
        let input = GreetInput { name: "".into(), title: None };
        let result = input.validate();
        assert!(!result.is_valid());
        assert!(result.errors.iter().any(|e| e.field == "name" && e.code == "VAL_REQUIRED"));
    }

    #[test]
    fn test_validate_name_max_length() {
        let input = GreetInput { name: "a".repeat(101), title: None };
        let result = input.validate();
        assert!(!result.is_valid());
    }

    #[test]
    fn test_sanitize_trims_whitespace() {
        let input = GreetInput { name: "  你好  ".into(), title: None };
        let sanitized = Unvalidated::new(input).sanitize();
        assert_eq!(sanitized.as_ref().name, "你好");
    }

    #[test]
    fn test_type_state_pipeline_success() {
        let input = GreetInput { name: "Penrose".into(), title: Some("开发者".into()) };
        let validated = Unvalidated::new(input)
            .sanitize()
            .validate()
            .expect("校验应通过");
        assert_eq!(validated.as_ref().name, "Penrose");
    }
}
```

### 13.3 集成测试（tests/integration.rs）

```rust
use feature_hello::FeatureHello;
use system_core::*;

#[test]
fn test_greet_command_ok() {
    let mut module = FeatureHello {
        hasher: None, encryptor: None, signer: None, keystore: None,
    };
    module.init(serde_json::json!({})).unwrap();
    let ctx = ExecutionContext::new_test();

    let result = module.execute("greet", serde_json::json!({
        "name": "Penrose"
    }), &ctx);

    assert!(result.is_ok(), "greet 命令应成功");
    let output: serde_json::Value = result.unwrap();
    assert!(output["message"].as_str().unwrap().contains("Penrose"));
}

#[test]
fn test_greet_unknown_command() {
    let mut module = FeatureHello {
        hasher: None, encryptor: None, signer: None, keystore: None,
    };
    module.init(serde_json::json!({})).unwrap();
    let ctx = ExecutionContext::new_test();

    let result = module.execute("nonexistent", serde_json::json!({}), &ctx);
    assert!(result.is_err());
    let err: ErrorPayload = serde_json::from_str(&result.unwrap_err()).unwrap();
    assert_eq!(err.code, "SYS_UNKNOWN_COMMAND");
}
```

### 13.4 测试命令

```bash
# 仅运行本模块测试
cargo test -p feature-<name>

# 全仓测试（确认没破坏其他模块）
cargo test --workspace
```

---

## 14. 模块组合

多模块协作时使用编排原语（§12.28）。

### 14.1 Pipe（顺序串联）

```rust
use system_core::orchestration::Pipe;

let result = Pipe::new()
    .then(validate_module, "validate_order", order_json)
    .then(payment_module, "charge", payment_json)
    .then(notification_module, "send_email", email_json)
    .execute(&ctx)?;
// 任何一步失败，后续步骤取消
```

### 14.2 FanOut（并行分发）

```rust
use system_core::orchestration::FanOut;

let results = FanOut::new()
    .dispatch(&[
        (audit_module, "log_access", log_json),
        (metrics_module, "increment_counter", counter_json),
        (cache_module, "invalidate", cache_key_json),
    ])
    .execute(&ctx)?;
// 并行执行，等待全部完成
```

### 14.3 实验性编排数据契约（非运行时）

默认模块不得假设 orchestration primitives 可用。仅在受控实验中显式声明：

```toml
[dependencies]
system-core = { workspace = true, features = ["experimental-orchestration"] }
```

```rust
use system_core::experimental::{ModuleOp, SagaStep};

let step = SagaStep {
    forward: ModuleOp {
        module_name: "orders".into(),
        command: "create_order".into(),
        payload: order_json,
        timeout_ms: None,
    },
    compensate: None,
};
```

这只构造内存数据，不会执行步骤或补偿，也不证明 durable workflow、outbox/inbox、重放、幂等、崩溃恢复或 `UnknownOutcome`。任何运行时语义必须由后续 ADR 单独批准。

### 14.4 跨模块数据访问边界

```
┌─────────────┐                    ┌─────────────┐
│  模块 A      │                    │  模块 B      │
│             │  ✗ 禁止直接 SQL    │             │
│  SELECT *   │──── 查模块B的 ────▶│  orders      │
│             │      表            │  table      │
│             │                    │             │
│             │  ✓ 正确            │             │
│  execute(   │──────────────────▶│  execute()  │
│  "query_   │   通过命令通信      │             │
│   orders") │                    │             │
└─────────────┘                    └─────────────┘
```

模块 A 禁止直接查询模块 B 的数据库表（§12.23）。所有跨模块数据访问必须通过目标模块的 `execute()` 命令。

---

## 15. （可选）提供前端 UI

如果你的模块需要在系统中展示界面，创建 `frontend/` 目录。

### 目录结构

```
features/feature_<name>/
├── frontend/
│   ├── manifest.json       # 必填：声明路由与菜单
│   └── index.vue           # 必填：Vue 页面组件
```

系统启动时 `import.meta.glob` 自动扫描所有 `features/*/frontend/manifest.json`，生成路由并注入导航菜单，无需手动改路由配置。

---

### 15.1 manifest.json

```json
{
  "route": "/modules/<name>",
  "title": "<页面标题>",
  "icon": "i-ri:apps-2-line",
  "sort": 10,
  "auth": "admin"
}
```

| 字段 | 必须 | 说明 |
|------|------|------|
| `route` | 是 | 路由路径。子路径不以 `/` 开头会自动转为绝对路径 |
| `title` | 是 | 页面标题（导航、标签页、面包屑） |
| `icon` | 否 | 图标（iconify 格式，如 `i-ri:test-tube-line`） |
| `sort` | 否 | 排序权重，越大越靠前 |
| `auth` | 否 | 权限控制（string 或 string[]）。不设 = 登录即可访问 |
| `group` | 否 | 导航分组名，同组模块归入同一菜单 |

需免登录访问时，路由需加 `whiteList: true`（在系统路由层配置）。

---

### 15.2 index.vue 骨架

```vue
<script setup lang="ts">
const { metadata, execute, schema } = useModule()

// 调用模块命令
const result = await execute('your_command', { field: 'value' })
</script>

<template>
  <div>
    <FaPageHeader :title="metadata.name" />
    <FaPageMain>
      <!-- 你的 UI -->
    </FaPageMain>
  </div>
</template>
```

`useModule()` 无须 import，由 `unplugin-auto-import` 自动注入。

---

### 15.3 设计系统约束

模块前端必须遵循项目的统一设计语言，禁止引入独立主题。

#### 15.3.1 色彩

**全部使用 OKLCH CSS 变量**，禁止硬编码十六进制色值（如 `#171717`、`rgb(255,0,0)`）：

| Token | 用途 |
|-------|------|
| `oklch(var(--background))` | 页面背景 |
| `oklch(var(--foreground))` | 正文文字 |
| `oklch(var(--primary))` | 主交互色 |
| `oklch(var(--muted))` / `oklch(var(--muted-foreground))` | 次级区域/文字 |
| `oklch(var(--border))` | 边框 |
| `oklch(var(--destructive))` | 危险操作 |
| `oklch(var(--secondary))` / `oklch(var(--secondary-foreground))` | 次要背景/文字 |
| `oklch(var(--card))` | 卡片背景 |

**用法示例**：
```html
<div class="bg-[oklch(var(--background))] text-[oklch(var(--foreground))]">
<div class="border border-[oklch(var(--border))] rounded-lg">
```

#### 15.3.2 组件优先级

1. **Fa\* 内建组件**（80+，自动注入）→ 首选，Pro 版核心价值
2. **Fa\* 手动 import**（`FaImageUpload`, `FaFileUpload`, `FaIconPicker`, `FaNumberField`）→ 仍是 Fa\*，仅需手动引用
3. **PrimeVue 组件**（DataTable、Column、Tag 等）→ 最后手段，仅 Fa\* 不覆盖时使用
4. **原生 HTML** → 仅在以上都不适用时使用

Element Plus 已从项目移除，**禁止使用任何 El\* 组件**。

#### 15.3.3 组件规范速查

**DataTable（表格）：**
```vue
<DataTable :value="items" striped-rows show-gridlines size="small" :pt="dt">
  <Column field="name" header="名称" sortable />
  <Column header="操作" :frozen="true" align-frozen="right">
    <template #body="{ data }">
      <FaButton variant="outline" size="sm" @click="handleEdit(data)">编辑</FaButton>
    </template>
  </Column>
</DataTable>
```

- 必须带 `:pt` 属性统一 OKLCH 风格（参见 §15.3.6 标准 `dt` 配置）
- `#body` slot 参数是 `{ data }`（非 ElTable 的 `{ row }`）
- 排序事件：`@sort` → `event.sortField` + `event.sortOrder`（`1`/`-1`/`0`）
- `v-model:selection` 直接双向绑定，无需 `@selection-change`

**FaButton（按钮）：**

| 场景 | 写法 | 渲染 |
|------|------|------|
| 表格操作列 | `size="sm"` | `h-8 px-3 text-sm` — 标准小按钮 |
| 纯图标按钮 | `size="icon-sm"` | `size-8` — 如 `<FaIcon>` |
| 危险操作 | `variant="destructive"` | 红色背景 |

> **禁止**：`size="icon-sm"` 搭配文字内容 — 32×32px 方形按钮会挤压文字。

**Badge/Tag（标签）：**
```html
<span class="inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ring-1 ring-inset
  bg-[oklch(var(--primary)/10%)] text-[oklch(var(--primary))] ring-[oklch(var(--primary)/20%)]">
  标签文字
</span>
```

- 形状：`rounded-full`（药丸形）
- 字号/字重：`text-xs font-medium`
- 6 种 severity 的颜色映射：

| severity | 背景 | 文字 | Ring |
|----------|------|------|------|
| success | `oklch(var(--primary)/10%)` | `oklch(var(--primary))` | `oklch(var(--primary)/20%)` |
| info | `oklch(var(--muted))` | `oklch(var(--muted-foreground))` | `oklch(var(--border))` |
| warn | `oklch(0.95 0.04 85)` | `oklch(0.55 0.15 55)` | 无 |
| danger | `oklch(var(--destructive)/10%)` | `oklch(var(--destructive))` | `oklch(var(--destructive)/20%)` |
| secondary | `oklch(var(--secondary))` | `oklch(var(--secondary-foreground))` | `oklch(var(--border))` |
| contrast | `oklch(var(--foreground))` | `oklch(var(--background))` | 无 |

**Form（表单）：**
```html
<div :class="{ 'opacity-50 pointer-events-none': loading }">
  <div class="max-w-600px space-y-6">
    <div>
      <label class="block text-sm font-medium leading-none mb-2">字段：</label>
      <FaInput v-model="form.field" class="w-full" />
    </div>
  </div>
</div>
```

- 表单验证使用 `zod.safeParse(form)` 直接验证（无需 vee-validate）
- 标签格式：`字段：`（中文冒号后缀）
- 加载态：`:class="{ 'opacity-50 pointer-events-none': loading }"`
- ~~`v-loading`~~ 不可用（Element Plus 指令，已移除）

**FaCard（卡片）：**
```html
<FaCard title="标题" description="描述文字">
  <!-- 内容 -->
</FaCard>
```

#### 15.3.4 页面骨架模式

**列表页（list.vue）：**
```
FaPageHeader
  └─ FaPageMain
       ├─ FaSearchBar
       │    └─ grid grid-cols-[repeat(auto-fit,minmax(300px,1fr))] gap-x-8 gap-y-2
       ├─ mx--5 my-4 border-t border-t-dashed           ← 全宽虚线分割线
       ├─ 操作栏 (flex-center-between)
       ├─ DataTable (带 :pt)
       └─ FaPagination
```

- 不需要 `<style scoped>` 块，全部 UnoCSS 原子类
- 搜索字段用 `FaLabel` 包裹

**详情页（detail.vue）：**
```
FaFixedBar(top)
  └─ FaPageHeader（动态标题 + 返回按钮）
FaPageMain
  └─ <div class="w-full"> → DetailForm（表单引用）
FaFixedBar(bottom)
  └─ FaButton（提交 / 取消）
```

**DetailForm：**
- 纯 HTML 布局（`<div>` + `<label>`），不用 `ElForm`
- `defineExpose({ submit() })` 暴露异步提交
- 编辑回填在 `onMounted` 中调用 API 获取详情

#### 15.3.5 禁止事项

| 禁止 | 原因 | 替代 |
|------|------|------|
| `ElTable`/`ElForm`/`ElTag` 等 | Element Plus 已移除 | DataTable / HTML + Fa\* |
| `v-loading` 指令 | Element Plus 指令 | `:class="{ 'opacity-50 pointer-events-none': loading }"` |
| `class="el-input__inner"` | 死 CSS | `class="w-full text-sm px-3 border rounded-md h-10"` |
| 硬编码色值 | 破坏暗色模式 | `oklch(var(--xxx))` token |
| `size="icon-sm"` + 文字 | 方形图标按钮挤压文字 | `size="sm"` |
| DataTable 无 `:pt` | Aura 原生样式与框架不一致 | 标准 `dt` 配置 |
| `<style scoped>` 在 list.vue | 冗余 | UnoCSS 原子类 |
| Shadow DOM / `attachShadow()` | PrimeVue 不支持，炸全局样式 | CSS @layer 或 `<iframe>` |
| `unstyled: true` | 全部手写 CSS，1人不可维护 | Styled 模式 + Aura preset |
| `isStandalone` / `import.meta.env` | 破坏双模 | CSS 变量自动适配 |
| 模块缺失 `fallback-theme.css` | 独立运行无样式 | 必须提供最小令牌集 |
| 编造不存在的 Fa* 组件（幻觉） | AI 常见错误 | 白名单交叉验证 |

#### 15.3.6 标准 DataTable pt 配置

```typescript
const dt = {
  root: { class: 'my-4' },
  header: { class: 'bg-[oklch(var(--muted)/50%)]' },
  headerCell: ({ props }: any) => ({
    class: `px-4 py-2.5 text-left text-xs font-medium text-[oklch(var(--muted-foreground))] select-none ${
      props.sortable ? 'cursor-pointer hover:bg-[oklch(var(--muted))] transition-colors duration-200' : ''
    }`,
  }),
  bodyRow: { class: 'border-t border-[oklch(var(--border))] transition-colors duration-150 cursor-pointer bg-[oklch(var(--background))] hover:bg-[oklch(var(--muted)/50%)]' },
  bodyCell: { class: 'px-4 py-2 text-[oklch(var(--foreground))]' },
}
```

---

#### 15.3.7 双模渲染机制

模块前端同一份 `.vue` 代码以两种模式运行，CSS 变量是双模通信桥梁，**组件代码零环境感知**：

| 模式 | 入口 | CSS 变量来源 |
|------|------|------------|
| **主系统模式** | `ModulePageShell` → 系统路由 | `packages/themes/index.ts` → `:root`（8 主题 × light/dark） |
| **独立运行模式** | `standalone.html` → 自行挂载 | `fallback-theme.css`（模块内置最小令牌集） |

模块目录须包含两个额外文件：

**`fallback-theme.css`** — 最小令牌集，使用 OKLCH 裸数值（`L C H` 空格分隔，**不带 `oklch()` 包裹**），确保 `oklch(var(--xxx)/50%)` 透明度操作可用：

```css
/* features/feature_<name>/frontend/fallback-theme.css — 最小令牌集，不可删减 */
:root {
  --background: 1 0 0;
  --foreground: 0.141 0.005 285.823;
  --primary: 0.21 0.006 285.885;
  --primary-foreground: 0.985 0 0;
  --secondary: 0.967 0.001 286.375;
  --secondary-foreground: 0.21 0.006 285.885;
  --muted: 0.967 0.001 286.375;
  --muted-foreground: 0.552 0.016 285.938;
  --accent: 0.967 0.001 286.375;
  --accent-foreground: 0.21 0.006 285.885;
  --destructive: 0.577 0.245 27.325;
  --border: 0.92 0.004 286.32;
  --input: 0.92 0.004 286.32;
  --ring: 0.705 0.015 286.067;
  --card: 1 0 0;
  --card-foreground: 0.141 0.005 285.823;
  --popover: 1 0 0;
  --popover-foreground: 0.141 0.005 285.823;
}
```

暗色模式通过 `.dark` 类覆盖变量值实现。

**`standalone.html`**：

```html
<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>{{模块标题}}</title>
  <link rel="stylesheet" href="./fallback-theme.css" />
</head>
<body class="bg-[oklch(var(--background))] text-[oklch(var(--foreground))]">
  <div id="app"></div>
  <script type="module" src="./main.ts"></script>
</body>
</html>
```

#### 15.3.8 Shadow DOM 禁止

PrimeVue **完全不支持 Shadow DOM**。全局 PrimeVue 样式不穿透 Shadow boundary，违背后果：

| 破坏点 | 具体表现 |
|--------|---------|
| PrimeVue 组件裸渲染 | shadow root 内所有组件无任何样式 |
| UnoCSS 原子类失效 | `flex`、`text-sm`、`bg-[oklch(var(--xxx))]` 全部不可用 |
| Fa* 组件样式丢失 | 80+ 内建组件依赖全局 UnoCSS 注入 |
| Portal 弹出层错乱 | Dialog 挂载到 `<body>`（shadow 外），定位断裂 |

**替代方案：CSS @layer**（PrimeVue 原生支持，一行配置）：

```ts
app.use(PrimeVue, {
  theme: { preset: Aura, options: { cssLayer: 'primevue' } }
})
```

若未来需完全 DOM/CSS 隔离，使用 `<iframe>`。

#### 15.3.9 Unstyled 模式禁止

始终使用 Styled 模式 + Aura preset。`unstyled: true` → 所有组件需手写 CSS → 1 人团队不可维护。主题定制通过 `definePreset` 修改 Design Token。

#### 15.3.10 组件幻觉检测

AI Agent 最致命的失败模式：编造不存在的 Fa* 组件（`FaDatePicker`、`FaRichText`、`FaTreeSelect` 等）或混淆 PrimeVue `<Button>` 与 FaButton。提交前白名单交叉验证：

```bash
# Fa* 组件白名单校验
for c in $(grep -roP 'Fa[A-Z]\w+' features/*/frontend/ --include="*.vue" | sort -u); do
  dir=$(echo "$c" | sed 's/^Fa//' | sed 's/\([a-z]\)\([A-Z]\)/\1-\2/g' | tr '[:upper:]' '[:lower:]')
  ls "packages/components/src/$dir/index.vue" 2>/dev/null \
    || echo "HALLUCINATED: $c"
done

# PrimeVue 原始组件名检测
grep -roP '<Button\b|<Dialog\b|<Dropdown\b|<Menu\b|<Toast\b|<Toolbar\b' \
  features/*/frontend/ --include="*.vue" && echo "WARN: 应使用 Fa* 内建组件"
```

#### 15.3.11 模块样式隔离边界

模块 `.vue` 文件只允许三种样式来源：
1. 全局 CSS 变量（`oklch(var(--xxx))`）
2. UnoCSS 原子类
3. Fa* 内建组件自带样式

额外 CSS 变量需求通过 `manifest.json` 声明：

```json
{
  "route": "/modules/hello",
  "title": "多语言问候",
  "cssVars": ["--mod-hello-accent", "--mod-hello-surface"]
}
```

禁止：`<style scoped>`、自建 CSS 变量作用域、`:root {}` 块、`@import` 外部样式表。

#### 15.3.12 转移层边界（壳注入契约）

模块通过 `useModule()` 获取壳注入的 Wasm 上下文，**禁止**绕过：

```ts
import { useModule } from '@/modules/useModule'
const { moduleName, execute, metadata, isReady } = useModule()
```

壳注入 4 个 key（仅系统模式）：`moduleName`、`moduleExecute`、`moduleMetadata`、`runtimeReady`。独立模式下 inject() 返回 fallback 默认值——`execute()` 抛出错误，但 UI 仍正常渲染。

禁止：
- 自建 `fetch()` / `axios` 实例（绕过统一 transport）
- 模块间直接 import `.ts` 文件（绕过 Wasm 桥接）
- `if (isStandalone)` 环境分支（破坏双模）

#### 15.3.13 PrimeVue 组件双模规则

PrimeVue 11 个 overlay 组件（Dialog、Popover、Select 等）默认 `appendTo: "body"`——双模兼容。但全局单例组件（`<Toast />`、`<ConfirmDialog />`、`<DynamicDialog />`）必须在 `App.vue` 和 `standalone.html` 中**双端声明**。组件选择：Fa* overlay（FaModal/FaDrawer/FaSelect）优先于 PrimeVue 原生 overlay。

#### 15.3.14 3D / GPU 渲染（前瞻）

如需 WebGL/WebGPU：canvas 不通过 Teleport 移动（用 `position: fixed`）；CSS 变量通过 `ShaderPrefs` 桥接对象传入 shader uniform（**禁止在 rAF 循环中每帧读取**）；manifest.json 声明 `requires: ["webgl2", "wasm-gpu"]` + `fallback` 降级组件；渲染循环使用裸变量（非 `ref()`）避免 Proxy 开销。

---

### 15.4 自动注入清单

以下依赖无需在组件中手动 `import`：

| 类别 | 自动可用 |
|------|---------|
| Vue API | `ref`, `reactive`, `computed`, `watch`, `onMounted`, `defineProps`, `defineExpose`… |
| 路由 | `useRouter`, `useRoute` |
| Store | `useAppPage`, `useAppSettingsStore`, `usePagination`… |
| Fa\* 组件 | `FaButton`, `FaCard`, `FaInput`, `FaSelect`, `FaPageHeader`, `FaPageMain`, `FaSearchBar`, `FaPagination`, `FaDropdown`, `FaToast`… |
| Fa\* composable | `useFaModal`, `useFaToast`, `useFaLoading`, `useFaDrawer` |
| 工具 | `eventBus` |
| PrimeVue | `DataTable`, `Column`, `Tag`, `InputText`, `Select`, `Button`（通过 `PrimeVueResolver` 自动导入） |
| 模块 API | `useModule`（仅在模块前端内可用） |

手动 import 例外：`FaImageUpload`, `FaFileUpload`, `FaIconPicker`, `FaNumberField`, `z from 'zod'`。

---

### 15.5 快速审计命令

```bash
# Element Plus 残留
grep -rE "v-loading|el-input__inner|ElTable|ElForm|ElTag|element-plus" features/ --include="*.vue"

# 硬编码色值
grep -rE '#[0-9a-fA-F]{3,6}|rgb\(' features/ --include="*.vue"

# <style scoped> 块
grep -r '<style scoped>' features/*/frontend/ --include="*.vue"

# DataTable 无 :pt
grep -rl '<DataTable' features/*/frontend/ --include="*.vue" | while read f; do
  grep -q ':pt=' "$f" || echo "MISSING :pt: $f"
done

# Shadow DOM API
grep -rn 'attachShadow\|customElements\.define\|ShadowRoot' --include="*.ts" --include="*.vue"

# Unstyled 模式
grep -rn 'unstyled\s*:\s*true' --include="*.ts"

# 环境分支侵入
grep -rn 'isStandalone\|import.meta.env' features/*/frontend/index.vue

# fallback-theme.css 缺失
for d in features/*/frontend/; do
  [ -f "$d/fallback-theme.css" ] || echo "MISSING: $d/fallback-theme.css"
done

# 幻觉 Fa* 组件
for c in $(grep -roP 'Fa[A-Z]\w+' features/*/frontend/ --include="*.vue" | sort -u); do
  dir=$(echo "$c" | sed 's/^Fa//' | sed 's/\([a-z]\)\([A-Z]\)/\1-\2/g' | tr '[:upper:]' '[:lower:]')
  ls "packages/components/src/$dir/index.vue" 2>/dev/null \
    || echo "HALLUCINATED: $c"
done
```

---

## 16. 开发环境插件与工具链

本节描述 Talos 项目的标准开发环境配置，确保模块开发者拥有相同的 AI 辅助能力和自动化工具。

### 16.1 Claude Code 插件（用户级）

以下插件安装在用户 `~/.claude/settings.json` 的 `enabledPlugins` 中，所有模块开发者应安装：

| 插件 | 来源 | 用途 |
|------|------|------|
| `superpowers@claude-plugins-official` | 官方市场 | brainstorming、writing-plans、systematic-debugging、TDD、code-review 等核心工作流 |
| `frontend-design@claude-plugins-official` | 官方市场 | 生产级前端界面设计 |
| `code-review@claude-plugins-official` | 官方市场 | PR 代码审查 |
| `context7@claude-plugins-official` | 官方市场 | 实时库/框架文档查询（MCP） |
| `playwright@claude-plugins-official` | 官方市场 | 浏览器自动化测试与 UI 验证（MCP） |
| `github@claude-plugins-official` | 官方市场 | GitHub 交互 |
| `security-guidance@claude-plugins-official` | 官方市场 | 安全审查 |
| `session-report@claude-plugins-official` | 官方市场 | 会话使用报告 |
| `skill-creator@claude-plugins-official` | 官方市场 | 技能创建与性能测量 |
| `commit-commands@claude-plugins-official` | 官方市场 | Git 提交、推送、PR 创建 |
| `rust-analyzer-lsp@claude-plugins-official` | 官方市场 | Rust LSP 支持 |
| `claude-plugin-pnpm@adddog-tools` | 第三方 | pnpm workspace lint/typecheck/test |
| `claude-mem@thedotmack` | 第三方 | 跨会话持久记忆与知识库 |
| `agent-harness-kit@agent-harness-kit-marketplace` | 第三方 | 多代理编排、基准测试、ADR |

**第三方市场注册**：
```json
{
  "extraKnownMarketplaces": {
    "agent-harness-kit-marketplace": {
      "source": { "source": "github", "repo": "tuanle96/agent-harness-kit" }
    },
    "thedotmack": {
      "source": { "source": "github", "repo": "thedotmack/claude-mem" }
    }
  }
}
```

### 16.2 Claude Code 插件（项目级）

项目 `.claude/settings.json` 中启用：

| 插件 | 用途 |
|------|------|
| `pyright-lsp@claude-plugins-official` | Python LSP（如项目含 Python 脚本） |

### 16.3 项目级技能（skills/）

以下 12 个技能文件位于项目 `skills/` 目录，通过 `Skill` 工具直接调用：

| 技能 | 触发场景 |
|------|---------|
| `fa-crud-page-generator` | 生成 CRUD 列表/详情页 |
| `fa-route-generator` | 生成路由配置 |
| `fa-form-builder` | 构建表单组件 |
| `fa-store-generator` | 创建 Pinia Store |
| `fa-i18n-manager` | 添加/更新 i18n 翻译 |
| `fa-slot-creator` | 创建插槽组件 |
| `fa-framework-settings` | 修改框架设置 |
| `fa-theme-customizer` | 自定义主题 |
| `fa-page-optimizer` | 优化已有页面 |
| `fa-feedback` | 同一功能修改 ≥3 次仍未达预期时触发 |
| `talos-rust-module` | 创建新的 Rust 模块 |
| `talos-context` | Talos 系统上下文管理 |

### 16.4 项目级技能（.claude/skills/）

| 技能 | 用途 |
|------|------|
| `ui-ux-pro-max` | UI/UX 设计智能：67 风格、96 调色板、57 字体配对、13 技术栈 |
| `translate-skills` | 批量翻译技能 SKILL.md 文件为中英文 |

### 16.5 MCP 服务器

| 服务器 | 安装方式 | 关键工具 |
|--------|---------|---------|
| **PrimeVue** | 本地安装于 `~/.claude/primevue-mcp/`，包装 `@primevue/mcp` | `search_components`、`get_component`、`suggest_component`、`get_component_props`、`validate_props` 等 40+ 工具 |
| **Context7** | 随 `context7` 插件自动安装 | `resolve-library-id`、`query-docs` |
| **Playwright** | 随 `playwright` 插件自动安装 | `browser_navigate`、`browser_snapshot`、`browser_take_screenshot` |
| **Claude-mem** | 随 `claude-mem` 插件自动安装 | `search`、`observation_add`、`knowledge-agent` |

PrimeVue MCP 安装参考：
```bash
mkdir -p ~/.claude/primevue-mcp
cd ~/.claude/primevue-mcp
npm init -y && npm install @primevue/mcp
```
然后在 Claude Code 中通过 `/mcp` 对话框手动启用。

### 16.6 关键 Skill 调用场景速查

| 你要做什么 | 调用哪个 Skill |
|-----------|--------------|
| 新建功能/组件 | 先用 `superpowers:brainstorming` 探索设计，再编码 |
| 多步骤复杂任务 | `superpowers:writing-plans` → `superpowers:executing-plans` |
| 生成 CRUD 页面 | `fa-crud-page-generator` |
| 创建 Rust 模块 | `talos-rust-module` |
| Bug 修复 | 先用 `superpowers:systematic-debugging` 定位根因 |
| 查询 PrimeVue 组件 | MCP `primevue:search_components` / `suggest_component` |
| 查询框架文档 | MCP `context7:query-docs`（先 `resolve-library-id`） |
| UI 风格设计 | `ui-ux-pro-max` |
| 验证 UI 变更 | MCP `playwright:browser_navigate` + `browser_snapshot` |
| 运行 lint/typecheck | `claude-plugin-pnpm:pnpm-workspace-filter` |
| 合并前审查 | `superpowers:requesting-code-review` |

模块写完后，如果你希望：

- **被 Vue 前端调用** → 确认 wasm 兼容 → 注册进 `wasm-frontend` → 编译 Wasm 包
- **被后端服务调用** → 确认 `talos-server` 的 Runtime 中已注册
- **被 AI Agent 发现** → 无需额外操作，`schema()` 已自动导出为 OpenAI Function Calling 格式
