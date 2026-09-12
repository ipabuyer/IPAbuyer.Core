# IPAbuyer.Core AI 编程指导（简要版）

IPAbuyer.Core：IPAbuyer 的跨平台核心库，使用 Rust 重写原 C# 版 `IPAbuyer.Core` 的业务逻辑（ipatool 集成、购买状态、已购买同步、下载队列、数据库、日志缓冲），编译为 cdylib DLL，由 WinUI 3 主应用（仓库 [ipabuyer/ipabuyer](https://github.com/ipabuyer/ipabuyer)，本机路径 `E:\ipabuyer`）通过 FFI 调用。

**本文件是基准文件，不允许修改；详细开发指南见 [DEVELOPMENT.md](./DEVELOPMENT.md)，该文件为详细开发内容，鼓励修改以同步最新开发进度。**

## 硬性约束

1. 本文件是基准文件，不允许修改；DEVELOPMENT.md 为详细开发内容，鼓励修改以同步最新开发进度。
2. 所有文件均以 UTF-8 格式存储、读取和修改；开发机终端为 GBK 代码页，注意输出乱码问题。
3. Core 不包含任何用户界面文案：对外的消息一律返回稳定键名/错误码 + 原始数据，由主应用按 `.resw` 本地化。
4. Windows 专属能力（LocalSettings、PasswordVault、ApplicationData 路径）不进入 Core；相关值由宿主经 FFI 参数传入。
5. FFI 边界只使用 C ABI：禁止 Rust 类型、`Result`、panic 穿越 FFI（导出函数必须 `catch_unwind` 包裹）；详见 DEVELOPMENT.md。
6. 内存所有权遵循"Rust 分配、宿主释放"：宿主用完必须调用对应的 free 导出函数。
7. ipatool 版本要求与命令规范必须与主仓库保持同步（当前要求 ipatool ≥ 2.5.0）；任一侧变更需同步另一侧文档。
8. 数据库 schema 与主应用兼容：`PurchasedApp`（`user_version` 2，已拥有已并入已购买）+ `SyncState`；数据库文件路径由宿主传入，Core 不自行定位。
9. 提交前必须通过 `cargo fmt --check`、`cargo clippy -- -D warnings`、`cargo test`。
10. 平台目标以 `x86_64-pc-windows-msvc`、`aarch64-pc-windows-msvc` 为准（与主应用一致），跨平台目标（Linux/macOS）按需扩展但不得破坏上述约束。

## 模块与职责

| 模块 | 职责 |
| --- | --- |
| `src/ipatool/` | ipatool 进程编排：命令构建、路径（宿主传入）、执行、JSON 输出解析 |
| `src/auth/` | 登录与双重验证流程的纯逻辑与结果分类 |
| `src/purchases/` | 购买结果解释、购买状态策略、已购买列表同步（分页、写入） |
| `src/downloads/` | 下载队列、下载输出解析 |
| `src/appcatalog/` | iTunes Search API 客户端与结果解析、开发者筛选 |
| `src/db/` | SQLite（rusqlite）：已购买记录、SyncState、schema 迁移 |
| `src/logging/` | 环形日志缓冲（对齐主应用 UiLogStore：上限 1000 条、等级、快照导出） |
| `src/ffi/` | C ABI 导出层：字符串与 JSON 编解码、错误上报、资源释放 |

测试账户：用户名 `test`、密码 `test`，购买/下载一律直接成功，不执行 list-purchases 同步（与主仓库一致）。
