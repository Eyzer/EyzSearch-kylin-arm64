//! search-tool 核心库：提取 → 索引 → 搜索
//!
//! 模块边界（并行开发约定，勿跨模块改文件）：
//!   extract/  文本提取（任务 #27，agent 维护）
//!   text.rs   编码检测与解码（任务 #27，agent 维护）
//!   seg.rs    分词内核（S3/S8 spike 定型）
//!   index/    tantivy 索引构建（R2：Writer 写侧独占 + IndexManager 只读）
//!   indexer/  扫描-提取-索引管线（R2 自 app/scanner.rs 迁入，GUI/CLI 共用）
//!   search/   三种搜索模式 + 查询映射 + 过滤/排序（子项 1）
//!   dsl/      nom 8 高级语法 parser
//!   config.rs  文件类型表 / 目录规则 / 定时配置（子项 3）
//!   preview.rs 词组统计 / 匹配列表 / 文档内再搜索 / 文件信息（子项 4）
//!   schedule.rs 定时更新三态调度（子项 3）
//!   watch.rs  文件监听实时更新

pub mod cache;
pub mod config;
pub mod dsl;
pub mod extract;
pub mod index;
pub mod indexer;
pub mod ocr;
pub mod pinyin;
pub mod preview;
pub mod schedule;
pub mod search;
pub mod seg;
pub mod service;
pub mod store;
pub mod synonym;
pub mod text;
pub mod watch;
