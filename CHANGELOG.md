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
5. FFI 导出层种子：`ipabuyer_core_version` / `ipabuyer_core_free_string`（catch_unwind 边界模式）
6. 质量门禁：114 个单元测试、`cargo fmt --check` 与 `cargo clippy -- -D warnings` 全绿
7. 工程配置：AGENTS.md（基准约束）、DEVELOPMENT.md（开发指南）、.gitattributes（LFS 与换行）、.gitignore；版本号约定为 `0.0.x`，每发版一次 x+1
