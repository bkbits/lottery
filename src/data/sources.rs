//! 数据源编排：福彩官网为主，500.com 补齐 2013 年以前的历史。

use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::draw::{Draw, merge_dedup};

use super::script;

pub const CWL_JS: &str = include_str!("../../assets/js/cwl.js");
pub const W500_JS: &str = include_str!("../../assets/js/w500.js");

/// 500.com 脚本里"回填到哪一年"的占位符。
const END_YEAR_PLACEHOLDER: &str = "__END_YEAR__";

/// 一次抓取的结果：合并后的数据 + 每个数据源的处理说明。
#[derive(Debug, Default)]
pub struct FetchOutcome {
    pub draws: Vec<Draw>,
    pub messages: Vec<String>,
}

impl FetchOutcome {
    pub fn message_text(&self) -> String {
        self.messages.join("；")
    }
}

fn is_leap(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// 当前年份（不引入 chrono，够用即可）。
fn current_year() -> u32 {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut days = (secs / 86_400) as u64;
    let mut year = 1970u32;
    loop {
        let len = if is_leap(year) { 366 } else { 365 };
        if days < len {
            return year;
        }
        days -= len;
        year += 1;
    }
}

/// 抓取全量历史数据。任一数据源失败都不影响另一个。
pub fn fetch_history() -> FetchOutcome {
    let mut messages = Vec::new();
    let mut all: Vec<Draw> = Vec::new();
    let mut primary_ok = false;

    match script::run_script("福彩官网", CWL_JS) {
        Ok(draws) => {
            primary_ok = !draws.is_empty();
            messages.push(format!("福彩官网 {} 期", draws.len()));
            all.extend(draws);
        }
        Err(err) => messages.push(format!("福彩官网失败（{err:#}）")),
    }

    // 福彩官网最早只到 2013 年：缺早期数据时用 500.com 逐年回填。
    let earliest_year = all.iter().map(Draw::year).min().unwrap_or(u32::MAX);
    if earliest_year > 2003 {
        // 主数据源正常时只需补到 2012 年；否则补齐到当前年份。
        let end_year = if primary_ok { 2012 } else { current_year() };
        let script_text = W500_JS.replace(END_YEAR_PLACEHOLDER, &end_year.to_string());
        match script::run_script("500.com", &script_text) {
            Ok(draws) => {
                messages.push(format!("500.com {} 期", draws.len()));
                all.extend(draws);
            }
            Err(err) => messages.push(format!("500.com 失败（{err:#}）")),
        }
    }

    FetchOutcome {
        draws: merge_dedup(all),
        messages,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_year_is_plausible() {
        let year = current_year();
        assert!((2024..=2100).contains(&year), "{year}");
    }

    #[test]
    fn placeholder_is_present() {
        assert!(W500_JS.contains(END_YEAR_PLACEHOLDER));
    }

    #[test]
    #[ignore = "需要真实网络"]
    fn live_fetch_smoke() {
        script::set_fetcher(|url| {
            super::super::http::get(url)
                .map(|r| r.text)
                .map_err(|e| format!("{e:#}"))
        });

        let started = std::time::Instant::now();
        let outcome = fetch_history();
        println!("耗时 {:?}", started.elapsed());
        println!("数据源: {}", outcome.message_text());
        println!("合计 {} 期", outcome.draws.len());
        println!(
            "最新 {:?}",
            outcome.draws.first().map(|d| (d.code.clone(), d.line()))
        );
        println!(
            "最早 {:?}",
            outcome.draws.last().map(|d| (d.code.clone(), d.line()))
        );
        assert!(outcome.draws.len() > 3400, "历史数据应超过 3400 期");
    }
}
