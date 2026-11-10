//! 核心领域逻辑：数据模型、统计、预测。
//!
//! 这一层不依赖 UI、网络与数据库，便于单元测试。

pub mod draw;
pub mod predict;
pub mod stats;

pub use draw::{
    Draw, merge_dedup, normalize_code, normalize_date, parse_blue, parse_reds, sort_newest_first,
};
pub use predict::{BlueAlgo, PredictConfig, Prediction, RedAlgo, Rule};
pub use stats::{OmissionStat, StatsReport, analyze};
