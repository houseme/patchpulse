> Historical source proposal. The English task specifications in [docs/tasks](tasks/README.md), [architecture.md](architecture.md), and [api.md](api.md) supersede the example versions, dual-license statement, old date-library snippets, and service-registration example below. The implemented project is Apache-2.0 only and uses jiff with no chrono/time crate dependencies.

# PatchPulse 实战解决方案

> 一套面向 Windows Server 2019 的补丁健康服务完整落地方案，涵盖项目定义、架构设计文档与 AGENT.md 三部分。

---

# 一、项目名称与描述信息

## 1.1 基础元信息

| 字段             | 内容                                                                                       |
|:-----------------|:-------------------------------------------------------------------------------------------|
| **项目名称**     | PatchPulse                                                                                 |
| **中文名**       | 补丁脉搏                                                                                   |
| **仓库名**       | `patchpulse`                                                                               |
| **一句话描述**   | 面向 Windows Server 的轻量级补丁健康服务，采集已安装与待安装补丁并通过 HTTP 暴露统一视图。 |
| **项目类型**     | 常驻系统服务（Windows Service / 可选 Linux 守护进程）                                      |
| **实现语言**     | Rust 2021 Edition（MSRV 1.75+）                                                            |
| **许可证**       | Apache-2.0 OR MIT（双许可）                                                                |
| **目标平台**     | Windows Server 2016/2019/2022（x86_64），可扩展至 Windows 10/11                            |
| **默认监听端口** | `9100`                                                                                     |

## 1.2 长描述

PatchPulse 是一个用 Rust 编写的轻量级补丁健康服务，专为 Windows Server 2019 及同类系统设计。它定期采集本机的
**已安装补丁**（通过 WMI `Win32_QuickFixEngineering` 与 WUA COM）和 **待安装补丁**（通过 PowerShell `PSWindowsUpdate` 或
WUA COM），将结果缓存到内存快照中，并通过一组稳定的 HTTP 端点对外暴露。它同时输出 Prometheus 指标，可直接接入现有监控与告警体系。

相比 WSUS 这类集中式方案，PatchPulse 定位为 **单机侧的可观测性组件**
：它不负责分发补丁，只负责回答"这台机器当前补丁状态如何"。它可以与 WSUS、Azure Update
Manager、第三方补丁平台共存，作为补丁合规审计、告警联动和自动化运维的数据源。

## 1.3 关键词与标签

```
windows-server, patch-management, wmi, windows-update-agent, rust,
axum, prometheus, health-check, observability, compliance, wsus
```

## 1.4 设计目标与非目标

**目标**

- 单二进制、零外部运行时依赖（除 Windows 系统组件）
- 内存占用 < 30 MB，空闲 CPU 占用接近 0
- 采集失败不崩溃，保留上次成功快照并降级暴露
- HTTP API 稳定、可版本化、可被脚本与 Agent 直接消费
- 输出 Prometheus 指标，支持 `no_std` 风格的核心领域层便于单测

**非目标**

- 不实现补丁下载与安装（由 WUA / WSUS / 配置管理工具负责）
- 不做多机集中管理（由上游聚合层完成）
- 不实现 Web UI（仅提供 API 与指标）

---

# 二、架构设计文档

## 2.1 背景与问题陈述

Windows Server 2019 仍承载大量企业核心业务，每月安全更新是抵御漏洞利用与满足合规审计的必要手段。实际运维中常见以下痛点：

1. **状态不可见**：补丁是否安装、何时安装、是否重启待生效，缺乏机器可读的统一视图。
2. **采集方式割裂**：`Get-HotFix`、`Win32_QuickFixEngineering`、WUA COM、WSUS 报表各有覆盖范围，运维难以判断"到底缺哪些"。
3. **与监控体系脱节**：补丁状态通常停留在人工巡检层面，无法进入 Prometheus / 告警流水线。
4. **自动化困难**：PowerShell 脚本可写但难维护，缺少结构化输出、超时控制和错误降级。

PatchPulse 通过"采集 → 归一化 → 缓存 → 暴露"的固定管线解决上述问题，并把可观测性（日志、指标、健康探针）作为一等公民。

## 2.2 需求分析

### 2.2.1 功能性需求

| 编号 | 需求                                                                                   | 优先级 |
|:-----|:---------------------------------------------------------------------------------------|:-------|
| F1   | 定时采集已安装补丁（KB 编号、描述、安装时间）                                          | P0     |
| F2   | 定时采集待安装补丁（KB 编号、标题、类别、严重级别）                                    | P0     |
| F3   | 通过 HTTP 提供 `/health`、`/ready`、`/patches`、`/patches/pending`、`/patches/summary` | P0     |
| F4   | 输出 Prometheus 指标 `/metrics`                                                        | P0     |
| F5   | 支持配置文件与命令行参数                                                               | P1     |
| F6   | 支持注册为 Windows 服务                                                                | P1     |
| F7   | 采集失败时保留上次快照并标记陈旧                                                       | P0     |
| F8   | 支持采集后端开关（WMI / WUA / PowerShell）                                             | P1     |
| F9   | 结构化 JSON 日志                                                                       | P2     |

### 2.2.2 非功能性需求

| 维度     | 指标                                                    |
|:---------|:--------------------------------------------------------|
| 资源占用 | 常驻内存 < 30 MB，采集期 CPU < 5%                       |
| 采集时延 | 单次采集 < 5 s（WMI），< 20 s（PowerShell）             |
| 可用性   | 采集失败不影响 HTTP 服务可用性                          |
| 安全     | 默认只监听本地或由配置指定；不执行任意用户输入          |
| 可测试性 | 领域层与采集层解耦，采集层可在非 Windows 上以 mock 替换 |
| 可观测性 | 暴露采集耗时、成功/失败计数、快照时间戳                 |

## 2.3 总体架构

```
                          ┌───────────────────────────────────────────┐
                          │            PatchPulse 进程                 │
                          │                                           │
   ┌───────────┐          │  ┌──────────────┐    ┌────────────────┐  │
   │ Scheduler │──tick───▶│  │  Collector   │───▶│  Snapshot      │  │
   │ (tokio    │          │  │  Orchestrator│    │  Store         │  │
   │  interval)│          │  └──────┬───────┘    │ (RwLock<...>)  │  │
   └───────────┘          │         │            └───────┬────────┘  │
                          │         │                    │           │
                          │  ┌──────▼────────┐   ┌───────▼────────┐  │
                          │  │  Collectors   │   │   HTTP API     │  │
                          │  │  ┌──────────┐ │   │   (Axum)       │  │
                          │  │  │ WMI      │ │   │  /health       │  │
                          │  │  ├──────────┤ │   │  /ready        │  │
                          │  │  │ WUA COM  │ │   │  /patches      │  │
                          │  │  ├──────────┤ │   │  /patches/*    │  │
                          │  │  │ PowerShell│ │  │  /metrics      │  │
                          │  │  └──────────┘ │   └───────┬────────┘  │
                          │  └───────────────┘           │           │
                          └──────────────────────────────┼───────────┘
                                                         │
                                        ┌────────────────▼─────────────┐
                                        │  上游：Prometheus / 告警平台  │
                                        │        运维脚本 / Agent      │
                                        └──────────────────────────────┘
```

### 2.3.1 分层职责

| 层       | 模块                | 职责                                                                        |
|:---------|:--------------------|:----------------------------------------------------------------------------|
| 领域层   | `domain`            | 定义 `PatchRecord`、`PatchStatus`、`PatchSnapshot` 等纯数据模型，无平台依赖 |
| 采集层   | `collector`         | 实现 `Collector` trait，按后端（WMI / WUA / PowerShell）采集并归一化        |
| 缓存层   | `cache`             | 以 `Arc<RwLock<Snapshot>>` 保存最新快照，提供读写与陈旧判定                 |
| 调度层   | `scheduler`         | 按间隔触发采集，隔离阻塞调用，记录指标                                      |
| 服务层   | `api`               | Axum 路由与 Handler，将领域模型序列化为 JSON                                |
| 可观测层 | `observability`     | 日志初始化、Prometheus 指标注册                                             |
| 基础设施 | `config`、`service` | 配置加载、Windows 服务封装、CLI                                             |

### 2.3.2 关键数据流

1. `scheduler` 按 `installed_interval_secs` / `pending_interval_secs` 触发采集。
2. `CollectorOrchestrator` 并发调用各 `Collector`，每个 collector 在 `spawn_blocking` 中执行阻塞 IO。
3. 采集结果归一化为 `Vec<PatchRecord>`，合并去重（以 KB 编号为主键）。
4. 写入 `SnapshotStore`，更新 `last_refreshed`、`last_error`、`stale` 标志。
5. HTTP Handler 读取快照并返回 JSON；`/ready` 在首次成功采集前返回 503。

## 2.4 技术选型

| 关注点       | 选型                                      | 理由                                  |
|:-------------|:------------------------------------------|:--------------------------------------|
| 异步运行时   | `tokio`                                   | 生态成熟，与 Axum 天然集成            |
| HTTP 框架    | `axum` 0.7                                | 类型安全路由、Tower 中间件生态        |
| 序列化       | `serde` + `serde_json`                    | 事实标准                              |
| WMI 访问     | `wmi` crate                               | 封装 COM，支持反序列化与异步查询      |
| WUA COM      | `windows` crate                           | 官方绑定，可按需手动声明接口          |
| 阻塞任务隔离 | `tokio::task::spawn_blocking`             | 避免阻塞异步运行时                    |
| 日志         | `tracing` + `tracing-subscriber`          | 结构化、可切换 JSON                   |
| 指标         | `metrics` + `metrics-exporter-prometheus` | 轻量、与 Prometheus 对齐              |
| 配置         | `toml` + `clap`                           | 配置文件 + 命令行覆盖                 |
| Windows 服务 | `windows-service`                         | 官方推荐的服务封装                    |
| 错误处理     | `thiserror` + `anyhow`                    | 库层用 `thiserror`，应用层用 `anyhow` |

> **版本提示**：`wmi`、`windows`、`metrics` 与 `metrics-exporter-prometheus` 之间存在版本耦合，落地时请以 `cargo tree` 为准，统一
> `windows` crate 版本，避免重复链接。

## 2.5 目录结构

```
patchpulse/
├── Cargo.toml
├── AGENT.md
├── README.md
├── LICENSE
├── docs/
│   └── architecture.md
├── config/
│   └── patchpulse.toml
├── scripts/
│   ├── install-service.ps1
│   ├── uninstall-service.ps1
│   └── query-patches.ps1
├── src/
│   ├── main.rs
│   ├── app.rs
│   ├── config.rs
│   ├── domain/
│   │   ├── mod.rs
│   │   ├── patch.rs
│   │   └── snapshot.rs
│   ├── collector/
│   │   ├── mod.rs
│   │   ├── traits.rs
│   │   ├── orchestrator.rs
│   │   ├── wmi_installed.rs
│   │   ├── wua_pending.rs
│   │   └── powershell_pending.rs
│   ├── cache/
│   │   └── mod.rs
│   ├── scheduler/
│   │   └── mod.rs
│   ├── api/
│   │   ├── mod.rs
│   │   ├── routes.rs
│   │   ├── handlers.rs
│   │   └── error.rs
│   ├── observability/
│   │   ├── mod.rs
│   │   ├── logging.rs
│   │   └── metrics.rs
│   └── service/
│       └── windows_service.rs
└── tests/
    ├── api_contract.rs
    └── domain_merge.rs
```

## 2.6 领域模型

```rust
// src/domain/patch.rs
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatchStatus {
    Installed,
    Pending,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatchSource {
    Wmi,
    Wua,
    PowerShell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchRecord {
    /// 归一化后的 KB 编号，如 "KB5034441"；无 KB 时使用来源生成的稳定 ID
    pub kb_id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub severity: Option<String>,
    pub installed_on: Option<DateTime<Utc>>,
    pub status: PatchStatus,
    pub reboot_required: bool,
    pub source: PatchSource,
}

impl PatchRecord {
    /// 以 KB 编号为主键合并：优先保留信息更完整的记录
    pub fn merge_prefer_richer(self, other: PatchRecord) -> PatchRecord {
        if self.completeness() >= other.completeness() {
            self
        } else {
            other
        }
    }

    fn completeness(&self) -> u8 {
        let mut score = 0;
        if self.title.is_some() { score += 1; }
        if self.description.is_some() { score += 1; }
        if self.category.is_some() { score += 1; }
        if self.installed_on.is_some() { score += 2; }
        score
    }
}
```

```rust
// src/domain/snapshot.rs
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use super::patch::PatchRecord;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PatchSnapshot {
    pub installed: Vec<PatchRecord>,
    pub pending: Vec<PatchRecord>,
    pub last_refreshed: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub consecutive_failures: u32,
}

impl PatchSnapshot {
    pub fn is_stale(&self, now: DateTime<Utc>, stale_after_secs: i64) -> bool {
        match self.last_refreshed {
            None => true,
            Some(t) => (now - t).num_seconds() > stale_after_secs,
        }
    }

    pub fn latest_installed_kb(&self) -> Option<&str> {
        self.installed
            .iter()
            .filter_map(|p| p.installed_on.map(|d| (d, p.kb_id.as_str())))
            .max_by_key(|(d, _)| *d)
            .map(|(_, kb)| kb)
    }
}
```

## 2.7 采集层设计

### 2.7.1 Collector trait

```rust
// src/collector/traits.rs
use async_trait::async_trait;
use crate::domain::patch::PatchRecord;

#[derive(Debug, thiserror::Error)]
pub enum CollectError {
    #[error("collector disabled: {0}")]
    Disabled(&'static str),
    #[error("platform unsupported: {0}")]
    Unsupported(&'static str),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("backend error: {0}")]
    Backend(String),
    #[error("timeout after {0}s")]
    Timeout(u64),
    #[error("parse error: {0}")]
    Parse(String),
}

#[async_trait]
pub trait Collector: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn timeout_secs(&self) -> u64;
    async fn collect(&self) -> Result<Vec<PatchRecord>, CollectError>;
}
```

### 2.7.2 WMI 采集已安装补丁（主路径）

```rust
// src/collector/wmi_installed.rs
use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use serde::Deserialize;
use crate::domain::patch::{PatchRecord, PatchSource, PatchStatus};
use super::traits::{CollectError, Collector};

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct QuickFix {
    hot_fix_id: String,
    description: Option<String>,
    caption: Option<String>,
    installed_on: Option<String>, // WMI 返回 CIM_DATETIME 字符串
}

pub struct WmiInstalledCollector;

#[async_trait]
impl Collector for WmiInstalledCollector {
    fn name(&self) -> &'static str { "wmi_installed" }
    fn timeout_secs(&self) -> u64 { 60 }

    async fn collect(&self) -> Result<Vec<PatchRecord>, CollectError> {
        #[cfg(not(windows))]
        {
            return Err(CollectError::Unsupported("wmi_installed requires Windows"));
        }

        #[cfg(windows)]
        {
            let rows = tokio::task::spawn_blocking(|| -> Result<Vec<QuickFix>, CollectError> {
                let com = wmi::COMLibrary::new()
                    .map_err(|e| CollectError::Backend(format!("COM init: {e}")))?;
                let conn = wmi::WMIConnection::new(com.into())
                    .map_err(|e| CollectError::Backend(format!("WMI connect: {e}")))?;
                conn.raw_query::<QuickFix>(
                    "SELECT HotFixID, Description, Caption, InstalledOn \
                     FROM Win32_QuickFixEngineering"
                )
                    .map_err(|e| CollectError::Backend(format!("WMI query: {e}")))
            })
                .await
                .map_err(|e| CollectError::Backend(format!("join: {e}")))??;

            Ok(rows.into_iter().map(map_quickfix).collect())
        }
    }
}

fn map_quickfix(qf: QuickFix) -> PatchRecord {
    PatchRecord {
        kb_id: normalize_kb(&qf.hot_fix_id),
        title: qf.caption,
        description: qf.description,
        category: Some("QuickFixEngineering".into()),
        severity: None,
        installed_on: qf.installed_on.as_deref().and_then(parse_cim_datetime),
        status: PatchStatus::Installed,
        reboot_required: false,
        source: PatchSource::Wmi,
    }
}

fn normalize_kb(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.to_ascii_uppercase().starts_with("KB") {
        trimmed.to_ascii_uppercase()
    } else {
        format!("KB{}", trimmed.trim_start_matches(|c: char| !c.is_ascii_digit()))
    }
}

fn parse_cim_datetime(s: &str) -> Option<DateTime<Utc>> {
    // CIM_DATETIME 形如 20250312000000.000000+480
    let s = s.trim();
    if s.len() < 14 { return None; }
    let naive = NaiveDateTime::parse_from_str(&s[..14], "%Y%m%d%H%M%S").ok()?;
    Some(Utc.from_utc_datetime(&naive))
}
```

> **局限说明**：`Win32_QuickFixEngineering` **不包含** Windows 累积更新（LCU）与服务堆栈更新（SSU）。因此 PatchPulse 必须同时启用
> WUA 或 PowerShell 采集，否则补丁视图会显著不完整。这一点需要在 `/patches/summary` 中通过 `coverage` 字段明示。

### 2.7.3 WUA COM 采集待安装补丁

`windows` crate 对 WUA 的绑定覆盖有限，实践中有两种可行路径：

**路径 A（推荐，稳定）**：通过 COM `IDispatch` 调用 `Microsoft.Update.Session`，手动声明必要接口。

```rust
// src/collector/wua_pending.rs（骨架）
// 说明：完整实现需要在 windows crate 的 COM 支持下声明
// IUpdateSession / IUpdateSearcher / IUpdateCollection / IUpdate 接口。
// 以下展示调用顺序，具体 vtable 绑定请参考 windows-rs COM 指南。

pub struct WuaPendingCollector;

#[async_trait]
impl Collector for WuaPendingCollector {
    fn name(&self) -> &'static str { "wua_pending" }
    fn timeout_secs(&self) -> u64 { 120 }

    async fn collect(&self) -> Result<Vec<PatchRecord>, CollectError> {
        #[cfg(not(windows))]
        { return Err(CollectError::Unsupported("wua_pending requires Windows")); }

        #[cfg(windows)]
        {
            tokio::task::spawn_blocking(|| -> Result<Vec<PatchRecord>, CollectError> {
                // 1. CoInitializeEx(MTA)
                // 2. CoCreateInstance(CLSID_UpdateSession -> IUpdateSession)
                // 3. session.CreateUpdateSearcher() -> IUpdateSearcher
                // 4. searcher.set_ServerSelection(ssDefault)
                // 5. searcher.Search("IsInstalled=0 and IsHidden=0") -> IUpdateCollection
                // 6. 遍历集合，读取 Title / KBArticleIDs / MsrcSeverity / Categories
                // 7. 映射为 PatchRecord { status: Pending, source: Wua, .. }
                Err(CollectError::Backend("WUA COM 实现占位，见文档 2.7.3 路径 A/B".into()))
            })
                .await
                .map_err(|e| CollectError::Backend(format!("join: {e}")))?
        }
    }
}
```

**路径 B（务实，可快速上线）**：通过 PowerShell 调用 WUA 的 COM，只把 PowerShell 当作"COM 启动器"，避免在 Rust 中手写 vtable。

```powershell
# scripts/query-patches.ps1
param(
    [ValidateSet('Installed', 'Pending')]
    [string]$Mode = 'Pending'
)

$ErrorActionPreference = 'Stop'
$session = New-Object -ComObject Microsoft.Update.Session
$searcher = $session.CreateUpdateSearcher()
$criteria = if ($Mode -eq 'Pending')
{
    "IsInstalled=0 and IsHidden=0"
}
else
{
    "IsInstalled=1"
}
$result = $searcher.Search($criteria)

$out = foreach ($u in $result.Updates)
{
    [pscustomobject]@{
        KbIds = @($u.KBArticleIDs)
        Title = $u.Title
        Description = $u.Description
        MsrcSeverity = $u.MsrcSeverity
        Categories = @($u.Categories | ForEach-Object { $_.Name })
        RebootRequired = [bool]$u.RebootRequired
        IsInstalled = [bool]$u.IsInstalled
    }
}
$out | ConvertTo-Json -Depth 5 -Compress
```

### 2.7.4 PowerShell 采集待安装补丁

```rust
// src/collector/powershell_pending.rs
use async_trait::async_trait;
use serde::Deserialize;
use crate::domain::patch::{PatchRecord, PatchSource, PatchStatus};
use super::traits::{CollectError, Collector};

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct PsUpdate {
    kb_ids: Option<Vec<String>>,
    title: Option<String>,
    description: Option<String>,
    msrc_severity: Option<String>,
    categories: Option<Vec<String>>,
    reboot_required: Option<bool>,
}

pub struct PowerShellPendingCollector {
    pub script_path: std::path::PathBuf,
}

#[async_trait]
impl Collector for PowerShellPendingCollector {
    fn name(&self) -> &'static str { "powershell_pending" }
    fn timeout_secs(&self) -> u64 { 180 }

    async fn collect(&self) -> Result<Vec<PatchRecord>, CollectError> {
        let script = self.script_path.clone();
        let raw = tokio::task::spawn_blocking(move || {
            std::process::Command::new("powershell.exe")
                .args([
                    "-NoProfile", "-NonInteractive",
                    "-ExecutionPolicy", "Bypass",
                    "-File", &script.to_string_lossy(),
                    "-Mode", "Pending",
                ])
                .output()
        })
            .await
            .map_err(|e| CollectError::Backend(format!("join: {e}")))?
            .map_err(CollectError::Io)?;

        if !raw.status.success() {
            return Err(CollectError::Backend(
                String::from_utf8_lossy(&raw.stderr).into_owned(),
            ));
        }

        let text = String::from_utf8_lossy(&raw.stdout);
        let text = text.trim();
        if text.is_empty() || text == "null" {
            return Ok(vec![]);
        }

        let items: Vec<PsUpdate> = serde_json::from_str(text)
            .map_err(|e| CollectError::Parse(format!("{e}: {text:.200}")))?;

        Ok(items.into_iter().flat_map(map_ps_update).collect())
    }
}

fn map_ps_update(u: PsUpdate) -> Vec<PatchRecord> {
    let cats = u.categories.unwrap_or_default();
    let category = cats.first().cloned();
    let kb_ids = u.kb_ids.unwrap_or_default();

    if kb_ids.is_empty() {
        return vec![PatchRecord {
            kb_id: format!("NO-KB-{}", short_hash(u.title.as_deref().unwrap_or("unknown"))),
            title: u.title,
            description: u.description,
            category,
            severity: u.msrc_severity,
            installed_on: None,
            status: PatchStatus::Pending,
            reboot_required: u.reboot_required.unwrap_or(false),
            source: PatchSource::PowerShell,
        }];
    }

    kb_ids.into_iter().map(|kb| PatchRecord {
        kb_id: if kb.to_ascii_uppercase().starts_with("KB") {
            kb.to_ascii_uppercase()
        } else {
            format!("KB{}", kb)
        },
        title: u.title.clone(),
        description: u.description.clone(),
        category: category.clone(),
        severity: u.msrc_severity.clone(),
        installed_on: None,
        status: PatchStatus::Pending,
        reboot_required: u.reboot_required.unwrap_or(false),
        source: PatchSource::PowerShell,
    }).collect()
}

fn short_hash(s: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    format!("{:08x}", h.finish() as u32)
}
```

### 2.7.5 采集编排与合并

```rust
// src/collector/orchestrator.rs
use std::collections::HashMap;
use std::time::Instant;
use crate::domain::patch::{PatchRecord, PatchStatus};
use super::traits::{CollectError, Collector};

pub struct Orchestrator {
    installed_collectors: Vec<Box<dyn Collector>>,
    pending_collectors: Vec<Box<dyn Collector>>,
}

pub struct CollectOutcome {
    pub installed: Vec<PatchRecord>,
    pub pending: Vec<PatchRecord>,
    pub errors: Vec<(String, CollectError)>,
}

impl Orchestrator {
    pub fn new(
        installed_collectors: Vec<Box<dyn Collector>>,
        pending_collectors: Vec<Box<dyn Collector>>,
    ) -> Self {
        Self { installed_collectors, pending_collectors }
    }

    pub async fn run(&self) -> CollectOutcome {
        let (installed, errs_i) = run_group(&self.installed_collectors).await;
        let (pending, errs_p) = run_group(&self.pending_collectors).await;

        let mut errors = errs_i;
        errors.extend(errs_p);

        CollectOutcome {
            installed: dedupe(installed),
            pending: dedupe(pending),
            errors,
        }
    }
}

async fn run_group(group: &[Box<dyn Collector>]) -> (Vec<PatchRecord>, Vec<(String, CollectError)>) {
    let mut records = Vec::new();
    let mut errors = Vec::new();

    for c in group {
        let name = c.name();
        let timeout = c.timeout_secs();
        let started = Instant::now();

        let res = tokio::time::timeout(
            std::time::Duration::from_secs(timeout),
            c.collect(),
        ).await;

        let elapsed = started.elapsed().as_secs_f64();
        metrics::histogram!("patchpulse_collect_duration_seconds", "collector" => name)
            .record(elapsed);

        match res {
            Ok(Ok(mut v)) => {
                metrics::counter!("patchpulse_collect_success_total", "collector" => name).increment(1);
                records.append(&mut v);
            }
            Ok(Err(e)) => {
                metrics::counter!("patchpulse_collect_failure_total", "collector" => name).increment(1);
                errors.push((name.to_string(), e));
            }
            Err(_) => {
                metrics::counter!("patchpulse_collect_failure_total", "collector" => name).increment(1);
                errors.push((name.to_string(), CollectError::Timeout(timeout)));
            }
        }
    }

    (records, errors)
}

fn dedupe(records: Vec<PatchRecord>) -> Vec<PatchRecord> {
    let mut map: HashMap<String, PatchRecord> = HashMap::new();
    for r in records {
        map.entry(r.kb_id.clone())
            .and_modify(|existing| {
                let merged = existing.clone().merge_prefer_richer(r.clone());
                *existing = merged;
            })
            .or_insert(r);
    }
    let mut out: Vec<_> = map.into_values().collect();
    out.sort_by(|a, b| a.kb_id.cmp(&b.kb_id));
    out
}
```

> **状态一致性**：若某个 KB 同时出现在 installed 和 pending 结果中（例如采集时序竞态），在写入快照前应做一次交叉校正：installed
> 优先，从 pending 中移除。

## 2.8 缓存层设计

```rust
// src/cache/mod.rs
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::Utc;
use crate::domain::snapshot::PatchSnapshot;

#[derive(Clone)]
pub struct SnapshotStore {
    inner: Arc<RwLock<PatchSnapshot>>,
    stale_after_secs: i64,
}

impl SnapshotStore {
    pub fn new(stale_after_secs: i64) -> Self {
        Self {
            inner: Arc::new(RwLock::new(PatchSnapshot::default())),
            stale_after_secs,
        }
    }

    pub async fn read(&self) -> PatchSnapshot {
        self.inner.read().await.clone()
    }

    pub async fn replace(&self, installed: Vec<_>, pending: Vec<_>) {
        let mut g = self.inner.write().await;
        g.installed = installed;
        g.pending = pending;
        g.last_refreshed = Some(Utc::now());
        g.last_error = None;
        g.consecutive_failures = 0;
    }

    pub async fn record_failure(&self, err: String) {
        let mut g = self.inner.write().await;
        g.last_error = Some(err);
        g.consecutive_failures = g.consecutive_failures.saturating_add(1);
    }

    pub async fn is_ready(&self) -> bool {
        self.inner.read().await.last_refreshed.is_some()
    }

    pub async fn is_stale(&self) -> bool {
        let g = self.inner.read().await;
        g.is_stale(Utc::now(), self.stale_after_secs)
    }
}
```

## 2.9 调度层设计

```rust
// src/scheduler/mod.rs
use std::time::Duration;
use tokio::time::{interval, MissedTickBehavior};
use crate::cache::SnapshotStore;
use crate::collector::orchestrator::Orchestrator;

pub async fn run(
    store: SnapshotStore,
    orchestrator: std::sync::Arc<Orchestrator>,
    interval_secs: u64,
) {
    // 启动时立即执行一次，缩短冷启动窗口
    tick_once(&store, &orchestrator).await;

    let mut ticker = interval(Duration::from_secs(interval_secs));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        ticker.tick().await;
        tick_once(&store, &orchestrator).await;
    }
}

async fn tick_once(store: &SnapshotStore, orchestrator: &Orchestrator) {
    let outcome = orchestrator.run().await;

    if outcome.installed.is_empty() && outcome.pending.is_empty() && !outcome.errors.is_empty() {
        let msg = outcome.errors.iter()
            .map(|(n, e)| format!("{n}: {e}"))
            .collect::<Vec<_>>()
            .join("; ");
        tracing::warn!(error = %msg, "all collectors failed, keeping previous snapshot");
        store.record_failure(msg).await;
        return;
    }

    for (name, err) in &outcome.errors {
        tracing::warn!(collector = %name, error = %err, "collector failed");
    }

    // 交叉校正：installed 优先
    let installed_kbs: std::collections::HashSet<_> =
        outcome.installed.iter().map(|p| p.kb_id.clone()).collect();
    let pending: Vec<_> = outcome.pending.into_iter()
        .filter(|p| !installed_kbs.contains(&p.kb_id))
        .collect();

    store.replace(outcome.installed, pending).await;
    tracing::info!("snapshot refreshed");
}
```

## 2.10 HTTP API 设计

### 2.10.1 端点契约

| 方法 | 路径               | 状态码    | 说明                                            |
|:-----|:-------------------|:----------|:------------------------------------------------|
| GET  | `/health`          | 200       | 进程存活即返回 `{"status":"ok"}`                |
| GET  | `/ready`           | 200 / 503 | 首次采集成功返回 200                            |
| GET  | `/patches`         | 200       | 已安装补丁列表，支持 `?status=`、`?since=` 过滤 |
| GET  | `/patches/pending` | 200       | 待安装补丁列表                                  |
| GET  | `/patches/summary` | 200       | 汇总信息                                        |
| GET  | `/metrics`         | 200       | Prometheus 文本格式                             |
| GET  | `/version`         | 200       | 版本与构建信息                                  |

### 2.10.2 响应示例

```json
// GET /patches/summary
{
  "total_installed": 312,
  "total_pending": 7,
  "latest_installed_kb": "KB5043050",
  "latest_installed_at": "2025-09-12T02:14:33Z",
  "last_refreshed": "2025-10-01T08:30:00Z",
  "is_stale": false,
  "consecutive_failures": 0,
  "last_error": null,
  "coverage": {
    "wmi_installed": true,
    "wua_pending": false,
    "powershell_pending": true
  }
}
```

### 2.10.3 路由与 Handler

```rust
// src/api/routes.rs
use axum::{routing::get, Router};
use crate::cache::SnapshotStore;
use super::handlers;

pub fn build(store: SnapshotStore) -> Router {
    Router::new()
        .route("/health", get(handlers::health))
        .route("/ready", get(handlers::ready))
        .route("/version", get(handlers::version))
        .route("/patches", get(handlers::list_installed))
        .route("/patches/pending", get(handlers::list_pending))
        .route("/patches/summary", get(handlers::summary))
        .route("/metrics", get(handlers::metrics))
        .with_state(store)
}
```

```rust
// src/api/handlers.rs
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use crate::cache::SnapshotStore;
use crate::domain::patch::PatchRecord;

pub async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

pub async fn ready(State(store): State<SnapshotStore>) -> impl IntoResponse {
    if store.is_ready().await {
        (StatusCode::OK, Json(serde_json::json!({ "status": "ready" })))
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({ "status": "initializing" })))
    }
}

pub async fn version() -> impl IntoResponse {
    Json(serde_json::json!({
        "name": env!("CARGO_PKG_NAME"),
        "version": env!("CARGO_PKG_VERSION"),
        "target": std::env::consts::OS,
    }))
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub since: Option<String>, // RFC3339
}

#[derive(Serialize)]
pub struct ListResponse {
    pub count: usize,
    pub items: Vec<PatchRecord>,
}

pub async fn list_installed(
    State(store): State<SnapshotStore>,
    Query(_q): Query<ListQuery>,
) -> Json<ListResponse> {
    let snap = store.read().await;
    Json(ListResponse { count: snap.installed.len(), items: snap.installed })
}

pub async fn list_pending(State(store): State<SnapshotStore>) -> Json<ListResponse> {
    let snap = store.read().await;
    Json(ListResponse { count: snap.pending.len(), items: snap.pending })
}

#[derive(Serialize)]
pub struct Summary {
    total_installed: usize,
    total_pending: usize,
    latest_installed_kb: Option<String>,
    latest_installed_at: Option<String>,
    last_refreshed: Option<String>,
    is_stale: bool,
    consecutive_failures: u32,
    last_error: Option<String>,
}

pub async fn summary(State(store): State<SnapshotStore>) -> Json<Summary> {
    let snap = store.read().await;
    let latest = snap.latest_installed_kb().map(|s| s.to_string());
    let latest_at = snap.installed.iter()
        .filter_map(|p| p.installed_on)
        .max()
        .map(|d| d.to_rfc3339());

    Json(Summary {
        total_installed: snap.installed.len(),
        total_pending: snap.pending.len(),
        latest_installed_kb: latest,
        latest_installed_at: latest_at,
        last_refreshed: snap.last_refreshed.map(|d| d.to_rfc3339()),
        is_stale: store.is_stale().await,
        consecutive_failures: snap.consecutive_failures,
        last_error: snap.last_error.clone(),
    })
}

pub async fn metrics() -> impl IntoResponse {
    use metrics_exporter_prometheus::PrometheusHandle;
    // handle 通过 Extension 注入（见 main.rs）
}
```

> `/metrics` 需要 `PrometheusHandle`，实际实现中通过 `axum::Extension` 注入，或在 `main.rs` 中闭包捕获 handle 后组装路由。

## 2.11 配置设计

```toml
# config/patchpulse.toml
[server]
bind = "127.0.0.1:9100"
request_timeout_secs = 15

[collector]
# 统一调度间隔；如需区分 installed/pending，可拆分为两个 interval
interval_secs = 1800
enable_wmi_installed = true
enable_wua_pending = false
enable_powershell_pending = true
powershell_script = "scripts/query-patches.ps1"
collector_timeout_secs = 180

[cache]
stale_after_secs = 7200

[observability]
log_level = "info"
log_format = "json"     # json | pretty
metrics_enabled = true
```

```rust
// src/config.rs
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub collector: CollectorConfig,
    pub cache: CacheConfig,
    pub observability: ObservabilityConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub bind: String,
    #[serde(default = "default_timeout")]
    pub request_timeout_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CollectorConfig {
    #[serde(default = "default_interval")]
    pub interval_secs: u64,
    #[serde(default = "t")]
    pub enable_wmi_installed: bool,
    #[serde(default)]
    pub enable_wua_pending: bool,
    #[serde(default = "t")]
    pub enable_powershell_pending: bool,
    #[serde(default = "default_ps_script")]
    pub powershell_script: String,
    #[serde(default = "default_collector_timeout")]
    pub collector_timeout_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CacheConfig {
    #[serde(default = "default_stale")]
    pub stale_after_secs: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ObservabilityConfig {
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_log_format")]
    pub log_format: String,
    #[serde(default = "t")]
    pub metrics_enabled: bool,
}

fn t() -> bool { true }
fn default_timeout() -> u64 { 15 }
fn default_interval() -> u64 { 1800 }
fn default_ps_script() -> String { "scripts/query-patches.ps1".into() }
fn default_collector_timeout() -> u64 { 180 }
fn default_stale() -> i64 { 7200 }
fn default_log_level() -> String { "info".into() }
fn default_log_format() -> String { "json".into() }

impl Config {
    pub fn load(path: Option<&std::path::Path>) -> anyhow::Result<Self> {
        let text = match path {
            Some(p) => std::fs::read_to_string(p)?,
            None => String::from_utf8(include_bytes!("../config/patchpulse.toml").to_vec())?,
        };
        Ok(toml::from_str(&text)?)
    }
}
```

## 2.12 主程序与启动流程

```rust
// src/main.rs
mod api;
mod cache;
mod collector;
mod config;
mod domain;
mod observability;
mod scheduler;
#[cfg(windows)]
mod service;

use std::sync::Arc;
use anyhow::Context;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "patchpulse", version, about = "Windows patch health service")]
struct Cli {
    #[arg(short, long)]
    config: Option<std::path::PathBuf>,

    /// 以前台模式运行（不注册为 Windows 服务）
    #[arg(long, default_value_t = false)]
    foreground: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cfg = config::Config::load(cli.config.as_deref())?;

    observability::logging::init(&cfg.observability);
    let metrics_handle = observability::metrics::install(cfg.observability.metrics_enabled);

    let store = cache::SnapshotStore::new(cfg.cache.stale_after_secs);
    let orchestrator = Arc::new(build_orchestrator(&cfg));

    // 后台采集任务
    {
        let store = store.clone();
        let orch = orchestrator.clone();
        let interval = cfg.collector.interval_secs;
        tokio::spawn(async move { scheduler::run(store, orch, interval).await });
    }

    let router = api::routes::build(store.clone())
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let router = if let Some(h) = metrics_handle {
        router.route("/metrics", axum::routing::get(move || {
            let h = h.clone();
            async move { h.render() }
        }))
    } else {
        router
    };

    let listener = tokio::net::TcpListener::bind(&cfg.server.bind)
        .await
        .with_context(|| format!("bind {}", cfg.server.bind))?;

    tracing::info!(addr = %cfg.server.bind, "patchpulse listening");
    axum::serve(listener, router).await?;
    Ok(())
}

fn build_orchestrator(cfg: &config::Config) -> collector::orchestrator::Orchestrator {
    use collector::traits::Collector;

    let mut installed: Vec<Box<dyn Collector>> = Vec::new();
    let mut pending: Vec<Box<dyn Collector>> = Vec::new();

    if cfg.collector.enable_wmi_installed {
        installed.push(Box::new(collector::wmi_installed::WmiInstalledCollector));
    }
    if cfg.collector.enable_wua_pending {
        pending.push(Box::new(collector::wua_pending::WuaPendingCollector));
    }
    if cfg.collector.enable_powershell_pending {
        pending.push(Box::new(collector::powershell_pending::PowerShellPendingCollector {
            script_path: cfg.collector.powershell_script.clone().into(),
        }));
    }

    collector::orchestrator::Orchestrator::new(installed, pending)
}
```

## 2.13 可观测性设计

### 2.13.1 指标清单

| 指标名                                | 类型      | 标签             | 说明               |
|:--------------------------------------|:----------|:-----------------|:-------------------|
| `patchpulse_collect_duration_seconds` | histogram | `collector`      | 单次采集耗时       |
| `patchpulse_collect_success_total`    | counter   | `collector`      | 采集成功次数       |
| `patchpulse_collect_failure_total`    | counter   | `collector`      | 采集失败次数       |
| `patchpulse_snapshot_age_seconds`     | gauge     | —                | 快照距上次刷新时长 |
| `patchpulse_installed_patches`        | gauge     | —                | 已安装补丁数量     |
| `patchpulse_pending_patches`          | gauge     | —                | 待安装补丁数量     |
| `patchpulse_stale`                    | gauge     | —                | 1 表示快照陈旧     |
| `patchpulse_http_requests_total`      | counter   | `path`, `status` | HTTP 请求计数      |

### 2.13.2 建议告警规则

```yaml
groups:
  - name: patchpulse
    rules:
      - alert: PatchSnapshotStale
        expr: patchpulse_stale == 1
        for: 30m
        labels: { severity: warning }
        annotations:
          summary: "补丁快照超过 {{ $value }} 未刷新"

      - alert: PatchCollectorFailing
        expr: rate(patchpulse_collect_failure_total[15m]) > 0.5
        for: 15m
        labels: { severity: warning }

      - alert: HighPendingPatchCount
        expr: patchpulse_pending_patches > 20
        for: 2h
        labels: { severity: info }

      - alert: PatchServiceDown
        expr: up{job="patchpulse"} == 0
        for: 5m
        labels: { severity: critical }
```

## 2.14 部署方案

### 2.14.1 注册为 Windows 服务

`src/service/windows_service.rs` 使用 `windows-service` crate 实现 `ServiceMain`，将 `main`
中的启动逻辑封装为可复用函数，供前台模式与服务模式共用。服务名建议 `PatchPulse`，启动类型 `Automatic (Delayed Start)`，登录账户
`LocalSystem`（WMI 与 WUA 查询需要足够权限）。

```powershell
# scripts/install-service.ps1
param(
    [string]$BinaryPath = "C:\PatchPulse\patchpulse.exe",
    [string]$ConfigPath = "C:\PatchPulse\patchpulse.toml"
)

$svcName = "PatchPulse"
$display = "PatchPulse Patch Health Service"

if (Get-Service -Name $svcName -ErrorAction SilentlyContinue)
{
    Write-Host "Service already exists. Stopping and removing..."
    Stop-Service $svcName -Force -ErrorAction SilentlyContinue
    sc.exe delete $svcName | Out-Null
    Start-Sleep -Seconds 2
}

New-Service -Name $svcName `
  -BinaryPathName "`"$BinaryPath`" --config `"$ConfigPath`" --foreground" `
  -DisplayName $display `
  -StartupType Automatic `
  -Description "Collects Windows patch status and exposes it over HTTP."

# 配置失败自动重启
sc.exe failure $svcName reset= 86400 actions= restart/5000/restart/10000/restart/30000 | Out-Null

Start-Service $svcName
Get-Service $svcName
```

> 若使用 `windows-service` crate 的完整服务封装，`BinaryPathName` 中不需要 `--foreground`。此处给出的是"前台模式 + sc.exe
> 托管"的简化路径，落地时二选一即可。

### 2.14.2 防火墙与网络暴露

```powershell
# 仅允许监控网段访问 9100
New-NetFirewallRule -DisplayName "PatchPulse HTTP" `
  -Direction Inbound -Action Allow -Protocol TCP -LocalPort 9100 `
  -RemoteAddress 10.0.0.0/8
```

**默认绑定 `127.0.0.1:9100`**，需要跨机采集时再改为内网地址并配合防火墙白名单。

### 2.14.3 与现有工具链共存

| 现有工具             | 共存方式                                                    |
|:---------------------|:------------------------------------------------------------|
| WSUS                 | PatchPulse 只读采集本机状态，不干预 WSUS 审批与分发         |
| Azure Update Manager | 可作为 PatchPulse 的补充视图；PatchPulse 提供机器侧原始数据 |
| Prometheus           | 直接抓取 `/metrics`                                         |
| Ansible / SaltStack  | 调用 `/patches/summary` 做合规判定                          |
| 企业监控平台         | 通过 `/health`、`/ready` 做存活与就绪探针                   |

## 2.15 安全设计

1. **最小暴露面**：默认仅监听回环；开放外网需显式配置并加防火墙白名单。
2. **无写操作**：服务不安装补丁、不修改系统配置，只做只读查询。
3. **输入约束**：PowerShell 脚本路径来自配置文件而非请求参数，避免命令注入。
4. **权限控制**：以 `LocalSystem` 运行以满足 WMI/WUA 查询权限，但不暴露任何提权接口。
5. **依赖审计**：CI 中执行 `cargo audit` 与 `cargo deny`，锁定 `windows` crate 版本。
6. **日志脱敏**：不记录完整命令行与脚本内容，避免泄露环境信息。

## 2.16 错误处理与降级策略

| 场景                        | 行为                                                                         |
|:----------------------------|:-----------------------------------------------------------------------------|
| 单个 collector 超时         | 记录 `Timeout` 错误，其余 collector 结果照常写入                             |
| 全部 collector 失败         | 保留上次快照，递增 `consecutive_failures`，`/ready` 仍为 200（已有历史数据） |
| 首次采集即失败              | `/ready` 返回 503，`/patches` 返回空列表                                     |
| 快照超过 `stale_after_secs` | `is_stale=true`，指标 `patchpulse_stale=1`                                   |
| HTTP 请求超时               | 由 `tower_http::timeout::TimeoutLayer` 统一返回 408                          |
| 快照读取竞争                | `RwLock` 读多写少，写时 clone 出快照后释放锁，避免长持锁                     |

## 2.17 性能与容量

- **内存**：单快照约 300–800 条记录，每条 ~200 B，总计 < 200 KB；进程常驻 < 30 MB。
- **CPU**：采集间隔 30 分钟，单次 WMI 查询 < 1 s，PowerShell < 15 s，平均 CPU 占用可忽略。
- **并发**：HTTP 端点只读内存，QPS 可达数千；实际瓶颈在上游抓取频率。
- **锁竞争**：`RwLock` 写操作仅在快照替换时短暂持锁，读操作无阻塞。

## 2.18 测试策略

| 层级     | 范围                                  | 方式                                                                   |
|:---------|:--------------------------------------|:-----------------------------------------------------------------------|
| 单元测试 | 领域模型合并、KB 归一化、CIM 时间解析 | `#[cfg(test)]`，纯函数测试                                             |
| 契约测试 | HTTP 端点状态码与 JSON schema         | `tests/api_contract.rs`，用 `axum::body::to_bytes` + `serde_json` 断言 |
| 集成测试 | Orchestrator 超时与降级               | 注入 mock `Collector`，验证部分失败时的快照写入                        |
| 平台测试 | WMI / PowerShell 采集                 | 在 Windows CI Runner 上执行，标记为 `#[ignore]` 供手动触发             |
| 静态检查 | Clippy、fmt、audit                    | CI 全量执行                                                            |

```rust
// tests/domain_merge.rs 示例
#[test]
fn merge_prefers_richer_record() {
    use patchpulse::domain::patch::*;
    let a = PatchRecord {
        kb_id: "KB1".into(),
        title: None,
        description: None,
        category: None,
        severity: None,
        installed_on: None,
        status: PatchStatus::Installed,
        reboot_required: false,
        source: PatchSource::Wmi
    };
    let b = PatchRecord { title: Some("t".into()), installed_on: Some(chrono::Utc::now()), ..a.clone() };
    assert!(a.merge_prefer_richer(b).title.is_some());
}
```

> 为便于集成测试，建议在 `Cargo.toml` 中同时声明 `[lib]` 与 `[[bin]]`，将核心逻辑放入 lib，二进制只做装配。

## 2.19 风险与演进路线

| 风险                                     | 影响             | 缓解                                                                 |
|:-----------------------------------------|:-----------------|:---------------------------------------------------------------------|
| `Win32_QuickFixEngineering` 不含 LCU/SSU | 已安装视图不完整 | 默认同时启用 WUA/PowerShell 采集，`/patches/summary` 暴露 `coverage` |
| WUA COM 在 Rust 中实现复杂               | 开发成本高       | 先用 PowerShell 路径上线，WUA 作为可选增强                           |
| `windows` crate 版本冲突                 | 构建失败         | 统一版本，CI 中 `cargo tree -d` 检查                                 |
| 采集时服务器负载高                       | 影响业务         | 设置超时、错峰调度、采集间隔可配置                                   |
| 权限不足导致查询失败                     | 数据缺失         | 明确以 LocalSystem 运行，启动时做权限自检并记录                      |

**演进方向**

- v0.2：增加 `/patches/export?format=csv`，便于合规报表导出。
- v0.3：支持多机聚合模式（PatchPulse Agent + PatchPulse Hub）。
- v0.4：接入 OpenTelemetry，支持 trace 导出。
- v0.5：提供补丁基线比对（与目标 KB 列表 diff），输出合规结论。

---

# 三、AGENT.md

````markdown
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
.map_err(|e| CollectError::Backend(format!("WMI query failed: {e}")))?;

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

| 方法 | 路径 | 成功码 | 说明 |
|:---|:---|:---|:---|
| GET | `/health` | 200 | 进程存活 |
| GET | `/ready` | 200 / 503 | 首次采集完成 |
| GET | `/version` | 200 | 版本信息 |
| GET | `/patches` | 200 | 已安装补丁列表 |
| GET | `/patches/pending` | 200 | 待安装补丁列表 |
| GET | `/patches/summary` | 200 | 汇总信息 |
| GET | `/metrics` | 200 | Prometheus 文本格式 |

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
````

---

# 四、落地路线建议

| 阶段        | 目标           | 交付物                                                                                       |
|:------------|:---------------|:---------------------------------------------------------------------------------------------|
| **第 1 周** | 可运行骨架     | `domain` + `cache` + `api` + WMI 采集，能在 Windows 上 `cargo run` 并访问 `/patches/summary` |
| **第 2 周** | 补全待安装视图 | `query-patches.ps1` + `PowerShellPendingCollector`，`/patches/pending` 可用                  |
| **第 3 周** | 可观测与服务化 | Prometheus 指标、JSON 日志、Windows 服务安装脚本、告警规则                                   |
| **第 4 周** | 加固与交付     | 契约测试、CI 流水线、架构文档定稿、AGENT.md 评审                                             |

该方案在保持单机轻量的同时，为后续多机聚合、合规基线和 OpenTelemetry 接入预留了清晰的扩展点。核心取舍是： **先以 WMI +
PowerShell 打通全链路，把 WUA COM 作为可选增强**，这样能在最短时间内交付可用价值，同时避免在 COM 绑定的复杂性上阻塞进度。