//! IPAbuyer 跨平台核心库。
//!
//! 阶段 1：纯逻辑迁移（ipatool 命令构建、响应解析、购买状态策略、已购买页解析）。
//! 模块划分与迁移映射见仓库根目录 DEVELOPMENT.md。

pub mod ffi;
pub mod ipatool;
pub mod json;
pub mod purchases;
