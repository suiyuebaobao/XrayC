//! 本模块汇总 Xray 配置编译器的单元测试。
//! 具体场景按通用配置、入站和出站拆分，避免主库文件继续膨胀。
//! 测试模块只在测试构建中编译，不影响公开接口。

mod common;
mod config;
mod inbound;
mod outbound;
mod outbound_extra;
