//! 双色球历史数据分析与号码预测。
//!
//! 分层：
//! - [`core`]：数据模型、统计指标、预测算法（纯逻辑，可单测）
//! - [`data`]：数据源（Boa JS 脚本 + HTTP）、SeaORM/SQLite 持久化
//! - [`app`]：Slint 界面绑定

slint::include_modules!();

pub mod app;
pub mod core;
pub mod data;

pub fn run() -> Result<(), slint::PlatformError> {
    app::run()
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    slint::android::init(app).expect("Failed to init Slint Android backend");
    run().expect("Failed to run UI event loop");
}
