# 更新日志

## v0.0.1

1. 项目初始化：Rust 跨平台核心库（cdylib DLL），供 WinUI 3 主应用通过 FFI 调用
   1. 技术栈：edition 2024、rusqlite（bundled）、serde_json、ureq（rustls）、fancy-regex
   2. 目标平台：x86_64-pc-windows-msvc、aarch64-pc-windows-msvc
2. 阶段 1：纯逻辑迁移（行为对齐主仓库 C# 实现与既有测试用例）
   1. `json`：ipatool 输出的宽容 JSON 解析
   2. `ipatool::command_builder`：命令参数构建（标准开关、下载、list-purchases）
   3. `ipatool::response_parser`：邮箱提取、成功/失败判定、流归一化
   4. `purchases`：状态归一化（已拥有并入已购买）、状态策略、购买结果解释、list-purchases 页解析
3. 阶段 2：进程编排与业务服务
   1. `execution`：子进程执行器（stdin 即时关闭、超时/取消、Windows 进程树终止）
   2. `ipatool::client`：六个 ipatool 命令的客户端（登录 60s / 查询购买 2min / 下载无超时）
   3. `auth::login`：登录与双重验证结果分类、模拟账户（test/test）
   4. `purchases::sync_service`：已购买列表分页同步（单飞行守卫、进度与日志回调、持久化与页拉取经 trait 注入）
   5. `appcatalog`：iTunes Search 搜索（ureq）、结果解析、开发者筛选
4. 阶段 3：状态与运行时
   1. `db`：SQLite（迁移 0→1→2，已拥有并入已购买；SyncState 同步状态；批量标记）
   2. `downloads`：下载队列状态机、输出解析（请求许可阶段、进度去重、ANSI 清洗）、结果解析
   3. `logging`：环形日志缓冲（上限 1000 条、快照导出）
5. FFI 导出层：C ABI 封装全部业务模块与轮询式长任务句柄
   1. 基础：`version` / `free_string` / `last_error`；动作类返回 i32 状态码，复杂契约走 JSON
   2. 数据库 / 认证 / 搜索 / 购买 / 同步 / 下载队列全部导出；`auth_*`、`purchase` 结果附带详细日志（按 `detailed_log` 开关返回脱敏命令行与输出），`auth_info` 附带解析字段与 `is_account_missing`
6. 质量门禁：128 个测试（119 单元 + 9 FFI 端到端）、`cargo fmt --check` 与 `cargo clippy -- -D warnings` 全绿
7. 工程配置：AGENTS.md（基准约束）、DEVELOPMENT.md（开发指南）、.gitattributes（LFS 与换行）、.gitignore、tag.ps1 与 build.yml（打 `v*` 标签触发测试→构建→发布）；版本号约定为 `0.0.x`，每发版一次 x+1
8. 修复：同步服务 `list-purchases` 调用未携带加密密钥；下载队列详细日志重复上抛；CI arm64 交叉编译补充 LLVM/clang
9. 修复：`sync_create` 借用宿主字符串跨线程使用（use-after-free），现于派生工作线程前复制为 owned 字符串，并补端到端回归测试
