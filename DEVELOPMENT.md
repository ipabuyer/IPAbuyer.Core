# IPAbuyer.Core 开发指南

本文是 IPAbuyer.Core 的详细开发指南。[AGENTS.md](./AGENTS.md) 是基准文件，不允许修改；本文为详细开发内容，鼓励修改以同步最新开发进度。

## 目录

1. [项目概述](#1-项目概述)
2. [通用约束](#2-通用约束)
3. [技术栈与目标平台](#3-技术栈与目标平台)
4. [构建、检查与测试](#4-构建检查与测试)
5. [FFI 边界设计](#5-ffi-边界设计)
6. [模块划分与迁移映射](#6-模块划分与迁移映射)
7. [ipatool 命令参考](#7-ipatool-命令参考)
8. [数据库](#8-数据库)
9. [本地化原则](#9-本地化原则)
10. [测试策略](#10-测试策略)
11. [与主仓库的同步清单](#11-与主仓库的同步清单)
12. [参考链接](#12-参考链接)

## 1. 项目概述

IPAbuyer.Core 使用 Rust 重写主仓库中原 C# `IPAbuyer.Core` 与 `IPAbuyer.Core.Execution` 的业务逻辑，目标：

1. 跨平台：核心逻辑不绑定 Windows，未来可支持 Linux/macOS 宿主。
2. 高性能：进程编排与 JSON 解析零拷贝倾向，避免不必要的分配与序列化层级。
3. 边界清晰：Core 是无 UI、无 Windows 专属依赖的库，以 cdylib DLL 形式被 WinUI 3 主应用 P/Invoke 调用。

迁移原则：**分阶段替换**。先迁移纯逻辑（解析器、状态策略、命令构建），验证与 C# 行为一致后再迁移进程编排、数据库与下载队列；主应用通过 FFI 逐个切换调用点。

## 2. 通用约束

1. 所有文件以 UTF-8 存储；开发机终端默认 GBK 代码页（代码页 936），PowerShell/cmd 下注意中文输出乱码，Git Bash 下注意路径转换（如 `/if` 被转义）。
2. Core 不写任何用户可见文案：对外消息返回稳定键名（与主应用 `Resources.resw` 键名对齐，如 `PurchaseSync/Log/Start`）+ 结构化数据，由主应用本地化。
3. Windows 专属能力（LocalSettings、PasswordVault、ApplicationData、资源加载器）不进入 Core；配置值、文件路径、ipatool 可执行文件路径均由宿主经 FFI 参数传入。
4. 公共逻辑必须有单元测试（见[测试策略](#10-测试策略)）。
5. 提交前通过 `cargo fmt --check`、`cargo clippy -- -D warnings`、`cargo test`。

## 3. 技术栈与目标平台

- 语言：Rust（stable 工具链，edition 2024）；MSRV 通过 `Cargo.toml` 的 `rust-version` 声明并在 CI 固定。
- crate 类型：`crate-type = ["cdylib"]`，产物为 `ipabuyer_core.dll`（Windows）/ `libipabuyer_core.so`（Linux）。
- 依赖选型（首版）：
  - `rusqlite`（开启 `bundled` 特性，静态编译 SQLite，免系统依赖）
  - `serde` + `serde_json`（FFI JSON 编解码与 ipatool/iTunes 输出解析）
  - `thiserror`（内部错误类型）
  - 进程与线程使用标准库（`std::process`、`std::thread`、mpsc），首版不引入 tokio；确有异步需求再评估。
- 目标平台（与主应用一致）：`x86_64-pc-windows-msvc`、`aarch64-pc-windows-msvc`；Linux/macOS 目标按需添加，添加时不得引入 Windows 条件编译进公共逻辑。
- 输出目录：`target/{target-triple}/release/`；主应用构建时从该目录取 DLL 并映射为部署名（主仓库 csproj 负责）。

## 4. 构建、检查与测试

```bash
cargo fmt                                  # 格式化
cargo clippy --all-targets -- -D warnings  # 静态检查（警告即错误）
cargo test                                 # 单元测试
cargo build --release --target x86_64-pc-windows-msvc      # x64 DLL
cargo build --release --target aarch64-pc-windows-msvc     # ARM64 DLL
```

1. `Cargo.lock` 提交入库：本仓库产出被主应用直接消费的 DLL，需保证可复现构建。
2. 调试 FFI：可用 `cargo build` 产物配合主应用的 packaged 调试；Core 自身验证以 `cargo test` 与小型集成测试（`tests/`）为主，不依赖主应用。

### 版本管理

1. 版本号约定为 `0.0.x`：每发版一次 `x` 加 1（如首个发布版本为 `0.0.1`，下一次为 `0.0.2`）。
2. 发版流程：`Cargo.toml` 的 `version` 改为下一个 `0.0.x` → 补充 [CHANGELOG.md](./CHANGELOG.md) → 提交 → 打 `v0.0.x` 标签推送，触发 release workflow 构建 DLL。
3. `ipabuyer_core_version()` 导出函数返回的就是 `Cargo.toml` 的版本号（编译期注入），宿主可用于校验 DLL 版本。

### 产物与发布策略

1. **测试版**：本地执行 `cargo build --release`，将 `target/<target-triple>/release/ipabuyer_core.dll` 直接复制到主仓库的 `Include\` 目录供开发调试；文件命名带架构与版本（如 `ipabuyer-core-0.0.1-windows-amd64.dll`、`...-arm64.dll`），避免 x64/ARM64 互相覆盖。
2. **正式版**：随主应用发版使用的 DLL **必须由 GitHub Actions 构建产出**，本地构建产物不得进入正式发版：
   1. 本仓库提供 workflow（`.github/workflows/build.yml`）：push/PR 时执行格式化、静态检查与测试；push `v*` 标签时以 matrix 构建 x64 与 ARM64 两个 DLL，计算 SHA-256 校验和，统一附加到本仓库的 GitHub Release；
   2. 主仓库正式发版时从本仓库对应 Release 下载 DLL 进入 `Include/`，不使用手工构建副本；
   3. workflow 固定工具链版本并以 `cargo build --release --locked` 构建，保证可复现。
3. 主仓库集成 DLL 后，需在其 `.gitattributes` 为 `*.dll` 增加 Git LFS 规则（与内置 ipatool exe 同策略）。

## 5. FFI 边界设计

### 5.1 ABI 约定

1. 导出函数统一前缀 `ipabuyer_core_`，`#[no_mangle]` + `extern "C"`；建议经 `cbindgen` 生成 C 头文件供主应用核对签名。
2. 所有导出函数体必须用 `std::panic::catch_unwind` 包裹（`AssertUnwindSafe`），panic 一律转为错误码，禁止穿越 FFI。
3. 返回值：`i32` 状态码，`0` 表示成功，非零为错误码；详细错误描述通过 `ipabuyer_core_last_error()` 获取（返回 UTF-8 字符串，线程本地）。
4. 复杂数据以 **JSON 字符串** 跨边界（`serde_json`），字段即契约并写入本文档对应小节；性能敏感路径后续再引入结构体打包，但需保持 JSON 路径可用。

### 5.2 字符串与内存

1. 宿主 → Core：`*const u8` + 长度（或 NUL 结尾的 `*const c_char`），内容必须为 UTF-8；Core 不修改宿主内存。
2. Core → 宿主：返回 Rust 分配的 `*mut c_char`（NUL 结尾 UTF-8），宿主用毕必须调用 `ipabuyer_core_free_string` 释放；遵循"谁分配谁释放"，禁止跨边界 `free`。
3. 长生命周期对象（如下载队列句柄）：以不透明 `*mut` 句柄返回，配套 `_create` / `_destroy` 导出；宿主负责调用 destroy，且不得重复释放。

### 5.3 并发与取消

1. 导出函数为阻塞同步调用；主应用侧用 `Task.Run` 包装，不阻塞 UI 线程。
2. Core 内部长任务在独立线程执行，并通过**轮询接口**上报状态（如 `ipabuyer_core_sync_status(handle) -> JSON`，包含阶段、已同步数、总数、最近日志游标）；函数指针回调作为后续优化项，引入时必须文档化线程约束。
3. 取消：长任务句柄提供 `_cancel` 导出，Core 内部以原子标志 + 子进程终止实现；取消后任务资源仍需 `_destroy` 释放。

## 6. 模块划分与迁移映射

| C# 原模块（主仓库） | Rust 模块 | 迁移阶段 |
| --- | --- | --- |
| `Integration/Ipatool/IpatoolCommandBuilder` | `src/ipatool/command_builder.rs` | 1（纯逻辑） |
| `Integration/Ipatool/IpatoolResponseParser` | `src/ipatool/response_parser.rs` | 1 |
| `Serialization/JsonPayload` | `src/json/`（serde 封装） | 1 |
| `Services/Purchases/PurchaseStatusPolicy`、`PurchaseRecordStatus` | `src/purchases/status_policy.rs` | 1 |
| `Services/Purchases/PurchaseResponseInterpreter` | `src/purchases/response_interpreter.rs` | 1 |
| `Services/Purchases/OwnedAppsPageParser` | `src/purchases/owned_apps_page_parser.rs` | 1 |
| `Integration/Ipatool/IpatoolClient` + `Core.Execution` | `src/ipatool/client.rs`（进程编排） | 2 |
| `Services/Purchases/PurchaseSyncService` | `src/purchases/sync_service.rs` | 2 |
| `Services/Authentication/LoginService` | `src/auth/login.rs` | 2 |
| `Services/AppCatalog/*` | `src/appcatalog/` | 2 |
| `Data/PurchasedApps/*` | `src/db/`（rusqlite + 迁移） | 3 |
| `Services/Downloads/*` | `src/downloads/`（队列 + 输出解析） | 3 |
| `Logging/UiLogStore` | `src/logging/`（环形缓冲，上限 1000 条，快照导出） | 3 |
| `State/SessionState`、`Configuration/*` | 不迁移：会话与配置属宿主职责，值经 FFI 传入 | — |

## 7. ipatool 命令参考

与主仓库 [AGENTS.md](https://github.com/ipabuyer/ipabuyer/blob/dev/AGENTS.md) 保持同步；Core 接收宿主传入的 ipatool 可执行文件路径与 keychain-passphrase，不自行解析路径或读取配置。

| 用途 | 命令模板 |
| --- | --- |
| 登录 | `ipatool auth login --auth-code 双重验证码 --email 邮箱 --password 密码 --keychain-passphrase 加密密钥` |
| 查询登录状态 | `ipatool auth info --keychain-passphrase 加密密钥` |
| 退出登录 | `ipatool auth revoke` |
| 列出已拥有 | `ipatool list-purchases --max-results 每页数量(≤100) --page 页码 --keychain-passphrase 加密密钥 --format json --non-interactive --verbose` |
| 购买 | `ipatool purchase --bundle-identifier APPID --keychain-passphrase 加密密钥 --format json --non-interactive --verbose` |
| 下载 | `ipatool download --output 输出位置 --bundle-identifier APPID --keychain-passphrase 加密密钥 --format json --non-interactive --verbose` |

1. 所有命令追加 `--format json`；进程环境强制 `NO_COLOR=1`、`TERM=dumb`。
2. 子进程标准输入启动后立即关闭（防交互提示挂起）；下载命令不设超时，查询类命令超时由 Core 常量定义（对齐主应用：登录 60 秒、查询/购买 2 分钟）。
3. **版本要求：ipatool ≥ 2.5.0**（`list-purchases` 与破坏性更改依赖，见主仓库 ipatool 页"版本要求"卡片）；升级内置版本时主仓库与本文档同步更新。

## 8. 数据库

1. SQLite 文件路径由宿主传入；schema 与主应用完全兼容：
   - `PurchasedApp(Id, AppID, Account, Status)`，`UNIQUE(AppID, Account)`，索引 `(AppID, Account)`，`Status` 统一为 `purchased`（owned 已并入）；
   - `SyncState(Account PRIMARY KEY, LastSuccessSyncUtc, LastAttemptSyncUtc)`。
2. 迁移逻辑对齐主应用 `user_version`：0 → 1 状态归一化，1 → 2 owned 并入 purchased；Core 的迁移实现必须与 C# 行为逐条等价，并有迁移单元测试。
3. 账户与 AppID 沿用主应用归一化规则：`trim + to_lowercase`。
4. 批量写入使用单事务（对齐 `BulkMarkPurchased`）。

## 9. 本地化原则

1. Core 返回稳定键名 + 原始数据（如 `{"key":"PurchaseSync/Log/Completed","args":["100","1753"]}`），禁止内嵌任何语言的文案。
2. 键名必须与主应用 `Strings/{zh-Hans,en-US}/Resources.resw` 现有键对齐；新增功能先在主仓库补 resw 键，Core 再引用键名。
3. 错误码枚举与键名的映射表维护在本文件对应模块小节。

## 10. 测试策略

1. 纯逻辑（解析器、策略、命令构建、迁移）必须有 `cargo test` 单元测试；测试用例优先采用主应用 C# 测试项目的既有用例（主仓库 `IPAbuyer.Tests` 项目），保证行为一致。
2. `list-purchases` 解析测试使用真实输出样本：成功页（`level":"info"` + `apps[]` + `totalCount`）、空页（`count:0`）、错误页（`level":"error"` + `success:false`）。
3. 进程编排（阶段 2 起）通过注入假可执行文件（shell 脚本/批处理）做集成测试，不依赖真实 Apple 账户。
4. 测试账户 `test`/`test` 规则与主仓库一致：购买/下载直接成功、不同步。

## 11. 与主仓库的同步清单

以下任意一项变更，两侧文档与实现必须同步更新：

1. ipatool 版本要求与命令行参数。
2. 数据库 schema 与迁移语义。
3. 购买状态语义（状态集、归一化规则、STDQ/alreadyOwned 处理）。
4. mock 账户（`test`/`test`）行为。
5. 本地化键名契约（主仓库 resw ↔ Core 返回键名）。

## 12. 参考链接

- 主仓库：<https://github.com/ipabuyer/ipabuyer>
- ipatool 仓库：<https://github.com/majd/ipatool>
- iTunes Search API：`https://itunes.apple.com/search?term=...&entity=software&limit=...&country=...`
- rusqlite：<https://github.com/rusqlite/rusqlite>
- cbindgen：<https://github.com/mozilla/cbindgen>
