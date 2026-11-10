//! 双色球基础数据模型。

use serde::{Deserialize, Serialize};

pub const RED_COUNT: usize = 6;
pub const RED_MAX: u8 = 33;
pub const BLUE_MAX: u8 = 16;

/// 一期开奖记录。红球按升序保存。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draw {
    pub code: String,
    pub date: String,
    pub reds: [u8; RED_COUNT],
    pub blue: u8,
    pub sales: Option<i64>,
    pub pool: Option<i64>,
}

impl Draw {
    pub fn new(code: String, date: String, reds: [u8; RED_COUNT], blue: u8) -> Self {
        Self {
            code,
            date,
            reds,
            blue,
            sales: None,
            pool: None,
        }
    }

    pub fn red_text(&self, sep: &str) -> String {
        let parts: Vec<String> = self.reds.iter().map(|n| format!("{n:02}")).collect();
        parts.join(sep)
    }

    pub fn blue_text(&self) -> String {
        format!("{:02}", self.blue)
    }

    /// 复制用的单行格式：`01,02,03,04,05,06+08`
    pub fn line(&self) -> String {
        format!("{}+{}", self.red_text(","), self.blue_text())
    }

    pub fn year(&self) -> u32 {
        self.code
            .get(0..4)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    }

    pub fn seq(&self) -> u32 {
        self.code.get(4..).and_then(|s| s.parse().ok()).unwrap_or(0)
    }

    pub fn sort_key(&self) -> u64 {
        self.year() as u64 * 1000 + self.seq() as u64
    }
}

/// 把各数据源的期号统一成 7 位形式（YYYYNNN）。
///
/// - 福彩官网直接给 `2026128`
/// - 500.com 给 `26001`（2 位年份 + 3 位期序）
pub fn normalize_code(raw: &str) -> Option<String> {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    match digits.len() {
        7 => Some(digits),
        5 => Some(format!("20{digits}")),
        4 => Some(format!("20{:0>3}", &digits[1..])),
        _ => None,
    }
}

/// 把日期统一成 `YYYY-MM-DD`。
pub fn normalize_date(raw: &str) -> String {
    let trimmed = raw.trim();
    let head: String = trimmed.chars().take(10).collect();
    if head.len() == 10
        && head.as_bytes()[4] == b'-'
        && head.as_bytes()[7] == b'-'
        && head.chars().filter(char::is_ascii_digit).count() == 8
    {
        return head;
    }
    // 形如 2026/11/08 或 2026.11.8
    let mut parts = trimmed.split(|c: char| c == '/' || c == '.' || c == '-');
    let y = parts.next().unwrap_or("");
    let m = parts.next().unwrap_or("");
    let d = parts.next().unwrap_or("");
    let (Ok(y), Ok(m), Ok(d)) = (y.parse::<u32>(), m.parse::<u32>(), d.parse::<u32>()) else {
        return trimmed.to_string();
    };
    format!("{y:04}-{m:02}-{d:02}")
}

/// 解析红球文本（逗号/空格分隔），去重排序并校验范围。
pub fn parse_reds(raw: &str) -> Option<[u8; RED_COUNT]> {
    let mut nums: Vec<u8> = raw
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<u8>().ok())
        .filter(|n| (1..=RED_MAX).contains(n))
        .collect();
    if nums.len() != RED_COUNT {
        return None;
    }
    nums.sort_unstable();
    nums.dedup();
    if nums.len() != RED_COUNT {
        return None;
    }
    let mut out = [0u8; RED_COUNT];
    out.copy_from_slice(&nums);
    Some(out)
}

pub fn parse_blue(raw: &str) -> Option<u8> {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    let n: u8 = digits.parse().ok()?;
    (1..=BLUE_MAX).contains(&n).then_some(n)
}

/// 按开奖时间从新到旧排序。
pub fn sort_newest_first(draws: &mut [Draw]) {
    draws.sort_by(|a, b| b.sort_key().cmp(&a.sort_key()));
}

/// 合并多个数据源，期号去重（保留先出现的记录）。
pub fn merge_dedup(draws: Vec<Draw>) -> Vec<Draw> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(draws.len());
    for d in draws {
        if seen.insert(d.code.clone()) {
            out.push(d);
        }
    }
    sort_newest_first(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_variants() {
        assert_eq!(normalize_code("2026128").as_deref(), Some("2026128"));
        assert_eq!(normalize_code("26001").as_deref(), Some("2026001"));
        assert_eq!(normalize_code("03001").as_deref(), Some("2003001"));
    }

    #[test]
    fn parse_red_text() {
        assert_eq!(
            parse_reds("05,06,10,25,27,30").unwrap(),
            [5, 6, 10, 25, 27, 30]
        );
        assert!(parse_reds("05,06,10,25,27").is_none());
        assert!(parse_reds("05,06,10,25,27,34").is_none());
    }

    #[test]
    fn normalize_dates() {
        assert_eq!(normalize_date("2026-11-08(日)"), "2026-11-08");
        assert_eq!(normalize_date("2003-02-23"), "2003-02-23");
        assert_eq!(normalize_date("2026/1/8"), "2026-01-08");
    }
}
