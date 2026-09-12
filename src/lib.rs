//! IPAbuyer 跨平台核心库。
//!
//! 阶段 1：纯逻辑迁移（ipatool 命令构建、响应解析、购买状态策略、已购买页解析）。
//! 阶段 2：进程编排（`execution` + `ipatool::client`）、登录分类（`auth`）、
//!         同步服务（`purchases::sync_service`）、App Catalog（`appcatalog`）。
//! 模块划分与迁移映射见仓库根目录 DEVELOPMENT.md。

pub mod appcatalog;
pub mod auth;
pub mod execution;
pub mod ffi;
pub mod ipatool;
pub mod json;
pub mod purchases;
