> Current implementation instructions (2026-10-01): PatchPulse uses Rust edition 2024 and Rust 1.98.1. The project license is Apache-2.0 only; dependencies retain upstream notices. Use jiff and never add chrono/time crates. All new code comments must be English. Native WMI/WUA use official windows bindings rather than the wmi crate. Metrics are a process-local Prometheus registry. Only scheduler::tick_once publishes backend outcomes. Preserve per-backend data on failure. English task specifications in docs/tasks and docs/architecture.md supersede the illustrative implementation examples below. Windows runtime tests must not be claimed from cross-compilation.

# AGENT.md

本文件为 AI 编码助手与自动化工具提供 PatchPulse 项目的上下文、约束与操作指引。开始任何修改前请完整阅读。

## 项目概述

PatchPulse 是一个用 Rust 编写的 Windows 补丁健康服务。它定时采集本机 **已安装补丁**（WMI `Win32_QuickFixEngineering`、WUA
COM）与 **待安装补丁**（WUA COM、PowerShell），缓存为内存快照，并通过 HTTP 暴露查询接口与 Prometheus 指标。

- 目标平台：Windows Server 2016/2019/2022（x86_64）
- 开发平台：Windows 优先；领域层与 API 层应能在 Linux/macOS 上编译与测试
- 默认端口：9100
- 核心原则： **只读采集、失败降级、接口稳定、可观测优先**

## 快速命令

```bash
# 构建（当前平台）
cargo build

# 构建 release（Windows 目标）
cargo build --release --target x86_64-pc-windows-msvc

# 运行（前台，读取默认配置）
cargo run -- --foreground

# 指定配置
cargo run -- --config config/patchpulse.toml --foreground

# 测试
cargo test                      # 全部单测 + 契约测试
cargo test --lib                # 仅领域层
cargo test -- --ignored         # 需要 Windows 权限的采集测试

# 代码质量（提交前必须全绿）
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo audit
cargo deny check

# 文档
cargo doc --no-deps --open
```

## 目录结构与职责边界

```
src/
├── main.rs               # 仅装配：解析 CLI、加载配置、启动 HTTP 与调度
├── app.rs                # 应用组装辅助（可选的启动/关闭钩子）
├── config.rs             # 配置结构体与加载；禁止在此写业务逻辑
├── domain/               # 纯数据模型与领域规则，禁止依赖平台 API
│   ├── patch.rs
│   └── snapshot.rs
├── collector/            # 采集实现，按后端分文件
│   ├── traits.rs         # Collector trait 与 CollectError
│   ├── orchestrator.rs   # 并发编排、超时、去重、交叉校正
│   ├── wmi_installed.rs
│   ├── wua_pending.rs
│   └── powershell_pending.rs
├── cache/                # SnapshotStore，唯一快照持有者
├── scheduler/            # tokio interval 循环
├── api/                  # Axum 路由与 Handler
│   ├── routes.rs
│   ├── handlers.rs
│   └── error.rs
├── observability/        # 日志与指标初始化
└── service/              # Windows 服务封装（cfg(windows)）
```

**依赖方向**：`main` → `api` / `scheduler` → `cache` / `collector` → `domain`。禁止反向依赖。`domain` 不得引入 `windows`、
`wmi`、`axum`、`tokio` 之外的运行时依赖。

## 架构约束

1. **阻塞调用必须隔离**：WMI、COM、`std::process::Command` 一律在 `tokio::task::spawn_blocking` 中执行。禁止在 async
   函数中直接调用阻塞 API。
2. **采集失败不得 panic**：所有 collector 返回 `Result<_, CollectError>`。`unwrap()` / `expect()` 仅允许出现在测试与
   `main` 的启动阶段。
3. **快照写入串行化**：只有 `scheduler::tick_once` 可以调用 `SnapshotStore::replace` / `record_failure`。Handler 只读。
4. **HTTP Handler 保持无状态**：Handler 不执行采集，只读快照。任何耗时操作必须放入后台任务。
5. **接口向后兼容**：新增字段可以，删除或重命名字段需要版本化路径（如 `/v2/patches`）。
6. **平台条件编译**：所有 Windows 专有代码必须包裹在 `#[cfg(windows)]` 中，并提供非 Windows 的 stub 实现以便跨平台编译。
7. **日志与指标成对出现**：每个 collector 的执行路径必须同时产生日志与 `patchpulse_collect_*` 指标。

## 编码规范

### 命名

- 类型与 trait：`UpperCamelCase`
- 函数、变量、模块：`snake_case`
- 常量：`SCREAMING_SNAKE_CASE`
- 采集器结构体：`<Backend><Purpose>Collector`，如 `WmiInstalledCollector`

### 错误处理

- 库层（`domain`、`collector`）使用 `thiserror` 定义具体错误类型。
- 应用层（`main`、`scheduler`）使用 `anyhow` 并附加上下文。
- 错误信息必须包含 **可定位的上下文**（后端名、KB 编号、脚本路径）。

```rust
// 推荐
.map_err( | e| CollectError::Backend(format!("WMI query failed: {e}"))) ?;

// 禁止
.unwrap();
```

### 日志

使用 `tracing`，字段化输出，禁止字符串拼接：

```rust
// 推荐
tracing::warn!(collector = %name, error = %err, "collector failed");

// 禁止
println!("collector {} failed: {}", name, err);
```

日志级别约定：`error` 仅用于服务不可用；`warn` 用于采集失败与降级；`info` 用于快照刷新与启动；`debug` 用于采集明细。

### 指标

新增采集器时必须注册以下三个指标：

```rust
metrics::histogram!("patchpulse_collect_duration_seconds", "collector" => name).record(elapsed);
metrics::counter!("patchpulse_collect_success_total", "collector" => name).increment(1);
metrics::counter!("patchpulse_collect_failure_total", "collector" => name).increment(1);
```

### 测试要求

- 新增领域逻辑必须附带单元测试。
- 新增 HTTP 端点必须附带契约测试（状态码 + JSON 结构断言）。
- 修改采集器必须验证"部分失败时快照仍更新"的行为。
- 平台相关代码用 `#[cfg_attr(not(windows), ignore)]` 标记。

## 常见任务

### 新增一个采集器

1. 在 `src/collector/` 下新建文件，实现 `Collector` trait。
2. 在 `CollectError` 中确认是否需要新增错误变体。
3. 在 `main.rs::build_orchestrator` 中按配置注册。
4. 在 `config.rs` 中新增开关字段，并在 `config/patchpulse.toml` 中给出默认值。
5. 补充单元测试与 `coverage` 字段暴露（如涉及新的数据来源）。
6. 更新 `docs/architecture.md` 的采集层章节。

### 新增一个 HTTP 端点

1. 在 `src/api/handlers.rs` 中实现 Handler，返回 `Json<T>` 或 `impl IntoResponse`。
2. 在 `src/api/routes.rs` 中注册路由。
3. 在 `tests/api_contract.rs` 中增加契约测试。
4. 在本文档"端点契约"表中补充一行。
5. 若涉及新指标，同步更新告警规则示例。

### 调整采集频率

优先改配置文件；若需支持 installed/pending 差异化频率，需将 `scheduler::run` 拆分为两个独立任务，并在 `CollectorConfig` 中新增
`installed_interval_secs` / `pending_interval_secs`。

### 排查补丁视图不完整

按顺序检查：

1. `/patches/summary` 中的 `coverage` 字段，确认各后端是否启用。
2. 服务日志中的 `collector failed` 记录，确认是否为权限或超时问题。
3. 服务运行账户是否为 `LocalSystem`。
4. 目标机器是否安装了 `PSWindowsUpdate` 模块（仅 PowerShell 路径需要，但本项目的脚本走 WUA COM，不依赖该模块）。
5. 若已安装补丁数量远少于预期，检查是否只启用了 WMI（不含 LCU/SSU）。

## 端点契约

| 方法 | 路径               | 成功码    | 说明                |
|:-----|:-------------------|:----------|:--------------------|
| GET  | `/health`          | 200       | 进程存活            |
| GET  | `/ready`           | 200 / 503 | 首次采集完成        |
| GET  | `/version`         | 200       | 版本信息            |
| GET  | `/patches`         | 200       | 已安装补丁列表      |
| GET  | `/patches/pending` | 200       | 待安装补丁列表      |
| GET  | `/patches/summary` | 200       | 汇总信息            |
| GET  | `/metrics`         | 200       | Prometheus 文本格式 |

## 平台注意事项

- `wmi` crate 会拉入 `windows` crate，注意与 `windows-service`、`windows` 的版本一致性。修改依赖后执行 `cargo tree -d`
  检查重复。
- 非 Windows 平台编译时，`collector/wmi_installed.rs` 与 `collector/wua_pending.rs` 必须返回 `CollectError::Unsupported`
  ，不得直接 `compile_error!`。
- Windows 服务模式下工作目录为 `C:\Windows\System32`，配置中的相对路径（如 `scripts/query-patches.ps1`）会解析失败。
  **服务部署时必须使用绝对路径**。
- `PowerShell` 进程启动需 `-NoProfile -NonInteractive -ExecutionPolicy Bypass`，避免用户配置干扰。

## 禁止事项

- ❌ 在 `domain/` 中引入平台 API 或 HTTP 框架
- ❌ 在 HTTP Handler 中执行采集或阻塞操作
- ❌ 使用 `unwrap()` / `expect()` 处理可恢复错误
- ❌ 删除或重命名已发布的 JSON 字段
- ❌ 在未更新文档与测试的情况下修改端点契约
- ❌ 提交 `cargo clippy -D warnings` 不通过的代码
- ❌ 将采集脚本路径改为可由 HTTP 请求参数控制

## 提交前检查清单

- [ ] `cargo fmt --all -- --check` 通过
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` 通过
- [ ] `cargo test` 通过
- [ ] 新增/修改的端点已更新契约测试
- [ ] 新增/修改的配置项已更新 `config/patchpulse.toml` 与本文档
- [ ] `cargo tree -d` 无意外重复依赖
- [ ] 涉及平台相关改动已在 Windows 上验证
- [ ] 架构变更已同步到 `docs/architecture.md`

## 参考文档

- `docs/architecture.md`：完整架构设计、数据流、部署与告警规则
- `config/patchpulse.toml`：配置项与默认值
- `scripts/install-service.ps1`：Windows 服务安装脚本
- `scripts/query-patches.ps1`：WUA COM 补丁查询脚本