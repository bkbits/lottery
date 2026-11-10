//! 数据获取与持久化。

pub mod entity;
pub mod http;
pub mod script;
pub mod sources;
pub mod store;

pub use sources::{FetchOutcome, fetch_history};
pub use store::{Store, default_db_path};
