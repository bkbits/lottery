// release 版本不弹控制台窗口；debug 版本保留控制台以便看日志。
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> Result<(), slint::PlatformError> {
    lottery_core::run()
}
