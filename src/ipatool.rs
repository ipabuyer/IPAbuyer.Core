//! ipatool 集成：命令构建与响应解析。
//!
//! 阶段 1 仅含纯逻辑；进程编排（`IpatoolClient` 等价物）属阶段 2。

pub mod command_builder;
pub mod response_parser;
