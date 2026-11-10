//! 往期统计：遗漏、冷热、和值、跨度、比值、AC 值、连号/重号/同尾/邻号等。

use super::draw::{BLUE_MAX, Draw, RED_MAX};

/// 质数表（红球范围内）。
pub const PRIMES: [u8; 11] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31];

/// 冷热号统计窗口（最近多少期）。
pub const RECENT_WINDOW: usize = 30;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Red,
    Blue,
}

impl Zone {
    pub fn max(self) -> u8 {
        match self {
            Zone::Red => RED_MAX,
            Zone::Blue => BLUE_MAX,
        }
    }

    fn contains(self, draw: &Draw, n: u8) -> bool {
        match self {
            Zone::Red => draw.reds.contains(&n),
            Zone::Blue => draw.blue == n,
        }
    }
}

/// 单个号码的遗漏/连出统计。
#[derive(Clone, Debug, Default)]
pub struct OmissionStat {
    pub number: u8,
    /// 当前遗漏：距今有多少期没出现（0 = 最新一期就出现）
    pub current: u32,
    /// 平均遗漏：统计期数 / 出现次数
    pub average: f64,
    /// 最大遗漏：历史最长连续未出现期数
    pub max: u32,
    pub hits: u32,
    /// 当前连出期数
    pub streak: u32,
    /// 历史最大连出期数
    pub max_streak: u32,
    /// 最近窗口内出现次数
    pub recent_hits: u32,
}

impl OmissionStat {
    pub fn overdue_ratio(&self) -> f64 {
        if self.average <= 0.0 {
            0.0
        } else {
            self.current as f64 / self.average
        }
    }
}

/// 分布统计的一格。
#[derive(Clone, Debug)]
pub struct Bucket {
    pub label: String,
    pub count: u32,
    pub percent: f64,
}

/// 单期的全部形态指标。红球升序。
#[derive(Clone, Debug, Default)]
pub struct DrawMetrics {
    pub sum: u32,
    pub span: u32,
    pub odd: u32,
    pub big: u32,
    pub prime: u32,
    pub road0: u32,
    pub road1: u32,
    pub road2: u32,
    pub zone1: u32,
    pub zone2: u32,
    pub zone3: u32,
    pub ac: u32,
    pub average: f64,
    /// 连号组数（>=2 的连续段）
    pub consecutive_runs: u32,
    /// 最长连号长度
    pub max_consecutive: u32,
    /// 重号个数（与上期红球重复）
    pub repeat: u32,
    /// 邻号个数（上期号码 ±1，且不与重号重复）
    pub neighbor: u32,
    /// 同尾号个数（尾数相同的号码个数，仅计成组的）
    pub same_tail: u32,
    /// 尾数和
    pub tail_sum: u32,
}

impl DrawMetrics {
    pub fn odd_even_text(&self) -> String {
        format!("{}:{}", self.odd, 6 - self.odd)
    }

    pub fn big_small_text(&self) -> String {
        format!("{}:{}", self.big, 6 - self.big)
    }

    pub fn prime_text(&self) -> String {
        format!("{}:{}", self.prime, 6 - self.prime)
    }

    pub fn road_text(&self) -> String {
        format!("{}:{}:{}", self.road0, self.road1, self.road2)
    }

    pub fn zone_text(&self) -> String {
        format!("{}:{}:{}", self.zone1, self.zone2, self.zone3)
    }

    pub fn consecutive_text(&self) -> String {
        if self.consecutive_runs == 0 {
            "无".to_string()
        } else {
            format!(
                "{}组(最长{}连)",
                self.consecutive_runs, self.max_consecutive
            )
        }
    }

    pub fn same_tail_text(&self) -> String {
        if self.same_tail == 0 {
            "无".to_string()
        } else {
            format!("{}个", self.same_tail)
        }
    }
}

/// 完整统计报告。`metrics[i]` 与 `draws[i]`（新到旧）一一对应。
#[derive(Clone, Debug, Default)]
pub struct StatsReport {
    pub total: usize,
    pub latest_code: String,
    pub latest_date: String,
    pub oldest_code: String,
    pub oldest_date: String,
    pub reds: Vec<OmissionStat>,
    pub blues: Vec<OmissionStat>,
    pub metrics: Vec<DrawMetrics>,
    pub sum_avg: f64,
    pub sum_min: u32,
    pub sum_max: u32,
    pub span_avg: f64,
    pub ac_avg: f64,
    pub tail_sum_avg: f64,
    pub consecutive_rate: f64,
    pub repeat_avg: f64,
    pub neighbor_avg: f64,
    pub same_tail_avg: f64,
    pub sum_buckets: Vec<Bucket>,
    pub odd_even_buckets: Vec<Bucket>,
    pub big_small_buckets: Vec<Bucket>,
    pub prime_buckets: Vec<Bucket>,
    pub road_buckets: Vec<Bucket>,
    pub zone_buckets: Vec<Bucket>,
    pub ac_buckets: Vec<Bucket>,
    pub hot_reds: Vec<u8>,
    pub cold_reds: Vec<u8>,
    pub hot_blues: Vec<u8>,
    pub cold_blues: Vec<u8>,
    pub latest_hot_reds: Vec<u8>,
    pub latest_hot_blues: Vec<u8>,
}

impl StatsReport {
    pub fn red_stat(&self, n: u8) -> Option<&OmissionStat> {
        self.reds.iter().find(|s| s.number == n)
    }

    pub fn blue_stat(&self, n: u8) -> Option<&OmissionStat> {
        self.blues.iter().find(|s| s.number == n)
    }
}

/// 计算一个号码区间的遗漏与连出。
pub fn omission_stats(draws: &[Draw], zone: Zone) -> Vec<OmissionStat> {
    let total = draws.len();
    let mut out = Vec::with_capacity(zone.max() as usize);

    for number in 1..=zone.max() {
        let mut hits = 0u32;
        let mut first_hit: Option<usize> = None;
        let mut last_hit: Option<usize> = None;
        let mut max_gap = 0u32;
        let mut streak = 0u32;
        let mut max_streak = 0u32;
        let mut running_streak = 0u32;
        let mut recent_hits = 0u32;

        for (idx, draw) in draws.iter().enumerate() {
            let present = zone.contains(draw, number);
            if idx < RECENT_WINDOW && present {
                recent_hits += 1;
            }
            if present {
                hits += 1;
                if first_hit.is_none() {
                    first_hit = Some(idx);
                }
                if let Some(prev) = last_hit {
                    let gap = (idx - prev - 1) as u32;
                    max_gap = max_gap.max(gap);
                }
                last_hit = Some(idx);
                running_streak += 1;
                max_streak = max_streak.max(running_streak);
            } else {
                running_streak = 0;
            }
            if idx == 0 && present {
                // 从最新一期往旧数据数连出
                streak = draws
                    .iter()
                    .take_while(|d| zone.contains(d, number))
                    .count() as u32;
            }
        }

        let current = first_hit.map_or(total as u32, |i| i as u32);
        let trailing_gap = last_hit.map_or(0, |last| (total - 1 - last) as u32);
        max_gap = max_gap.max(trailing_gap).max(current);
        let average = if hits > 0 {
            total as f64 / hits as f64
        } else {
            total as f64
        };

        out.push(OmissionStat {
            number,
            current,
            average,
            max: max_gap,
            hits,
            streak,
            max_streak,
            recent_hits,
        });
    }

    out
}

/// 计算单期形态指标；`prev` 用于重号/邻号。
pub fn draw_metrics(draw: &Draw, prev: Option<&Draw>) -> DrawMetrics {
    let reds = &draw.reds;
    let mut m = DrawMetrics {
        sum: reds.iter().map(|&n| n as u32).sum(),
        span: reds.iter().max().copied().unwrap_or(0) as u32
            - reds.iter().min().copied().unwrap_or(0) as u32,
        ..Default::default()
    };
    m.odd = reds.iter().filter(|n| *n % 2 == 1).count() as u32;
    m.big = reds.iter().filter(|n| **n >= 17).count() as u32;
    m.prime = reds.iter().filter(|n| PRIMES.contains(*n)).count() as u32;
    m.road0 = reds.iter().filter(|n| **n % 3 == 0).count() as u32;
    m.road1 = reds.iter().filter(|n| **n % 3 == 1).count() as u32;
    m.road2 = reds.iter().filter(|n| **n % 3 == 2).count() as u32;
    m.zone1 = reds.iter().filter(|n| **n <= 11).count() as u32;
    m.zone2 = reds.iter().filter(|n| (12..=22).contains(*n)).count() as u32;
    m.zone3 = reds.iter().filter(|n| **n >= 23).count() as u32;
    m.tail_sum = reds.iter().map(|n| (n % 10) as u32).sum();
    m.average = m.sum as f64 / reds.len() as f64;
    m.ac = ac_value(reds);

    // 连号
    let mut run = 1u32;
    for w in reds.windows(2) {
        if w[1] == w[0] + 1 {
            run += 1;
            m.max_consecutive = m.max_consecutive.max(run);
        } else {
            if run >= 2 {
                m.consecutive_runs += 1;
            }
            run = 1;
        }
    }
    if run >= 2 {
        m.consecutive_runs += 1;
        m.max_consecutive = m.max_consecutive.max(run);
    }

    // 同尾号
    let mut tails = [0u8; 10];
    for n in reds {
        tails[(n % 10) as usize] += 1;
    }
    m.same_tail = tails.iter().filter(|c| **c >= 2).map(|c| *c as u32).sum();

    // 重号 / 邻号
    if let Some(prev) = prev {
        m.repeat = reds.iter().filter(|n| prev.reds.contains(n)).count() as u32;
        m.neighbor = reds
            .iter()
            .filter(|n| !prev.reds.contains(n))
            .filter(|n| {
                let n = **n;
                (n > 1 && prev.reds.contains(&(n - 1)))
                    || (n < RED_MAX && prev.reds.contains(&(n + 1)))
            })
            .count() as u32;
    }

    m
}

/// AC 值 = 任意两码差值去重后的个数 - (号码个数 - 1)。
pub fn ac_value(reds: &[u8]) -> u32 {
    let mut diffs = Vec::with_capacity(15);
    for i in 0..reds.len() {
        for j in i + 1..reds.len() {
            diffs.push(reds[j] - reds[i]);
        }
    }
    diffs.sort_unstable();
    diffs.dedup();
    (diffs.len() as u32).saturating_sub(reds.len() as u32 - 1)
}

fn tally(labels: impl Iterator<Item = String>) -> Vec<Bucket> {
    let mut map: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut total = 0u32;
    for label in labels {
        *map.entry(label).or_default() += 1;
        total += 1;
    }
    let mut out: Vec<Bucket> = map
        .into_iter()
        .map(|(label, count)| Bucket {
            label,
            count,
            percent: if total > 0 {
                count as f64 * 100.0 / total as f64
            } else {
                0.0
            },
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.label.cmp(&b.label)));
    out
}

fn sum_bucket(sum: u32) -> String {
    let (lo, hi) = match sum {
        0..=50 => (21, 50),
        51..=70 => (51, 70),
        71..=90 => (71, 90),
        91..=110 => (91, 110),
        111..=130 => (111, 130),
        131..=150 => (131, 150),
        _ => (151, 183),
    };
    format!("{lo}-{hi}")
}

fn average_of(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

/// 冷热号：按最近窗口出现次数排序。
fn hot_cold(stats: &[OmissionStat], take: usize) -> (Vec<u8>, Vec<u8>) {
    let mut sorted = stats.to_vec();
    sorted.sort_by(|a, b| {
        b.recent_hits
            .cmp(&a.recent_hits)
            .then_with(|| a.current.cmp(&b.current))
    });
    let hot = sorted.iter().take(take).map(|s| s.number).collect();
    let mut cold_sorted = sorted.clone();
    cold_sorted.sort_by(|a, b| {
        a.recent_hits
            .cmp(&b.recent_hits)
            .then_with(|| b.current.cmp(&a.current))
    });
    let cold = cold_sorted.iter().take(take).map(|s| s.number).collect();
    (hot, cold)
}

/// 生成完整报告。要求 `draws` 已按新到旧排序。
pub fn analyze(draws: &[Draw]) -> StatsReport {
    if draws.is_empty() {
        return StatsReport::default();
    }

    let reds = omission_stats(draws, Zone::Red);
    let blues = omission_stats(draws, Zone::Blue);
    let metrics: Vec<DrawMetrics> = draws
        .iter()
        .enumerate()
        .map(|(i, d)| draw_metrics(d, draws.get(i + 1)))
        .collect();

    let sums: Vec<f64> = metrics.iter().map(|m| m.sum as f64).collect();
    let spans: Vec<f64> = metrics.iter().map(|m| m.span as f64).collect();
    let acs: Vec<f64> = metrics.iter().map(|m| m.ac as f64).collect();
    let tails: Vec<f64> = metrics.iter().map(|m| m.tail_sum as f64).collect();
    let repeats: Vec<f64> = metrics.iter().map(|m| m.repeat as f64).collect();
    let neighbors: Vec<f64> = metrics.iter().map(|m| m.neighbor as f64).collect();
    let same_tails: Vec<f64> = metrics.iter().map(|m| m.same_tail as f64).collect();
    let with_consecutive = metrics.iter().filter(|m| m.consecutive_runs > 0).count() as f64;

    let (hot_reds, cold_reds) = hot_cold(&reds, 6);
    let (hot_blues, cold_blues) = hot_cold(&blues, 4);

    let latest = &draws[0];
    let oldest = &draws[draws.len() - 1];

    StatsReport {
        total: draws.len(),
        latest_code: latest.code.clone(),
        latest_date: latest.date.clone(),
        oldest_code: oldest.code.clone(),
        oldest_date: oldest.date.clone(),
        sum_avg: average_of(&sums),
        sum_min: sums.iter().cloned().fold(f64::INFINITY, f64::min) as u32,
        sum_max: sums.iter().cloned().fold(0.0, f64::max) as u32,
        span_avg: average_of(&spans),
        ac_avg: average_of(&acs),
        tail_sum_avg: average_of(&tails),
        consecutive_rate: with_consecutive * 100.0 / metrics.len() as f64,
        repeat_avg: average_of(&repeats),
        neighbor_avg: average_of(&neighbors),
        same_tail_avg: average_of(&same_tails),
        sum_buckets: tally(metrics.iter().map(|m| sum_bucket(m.sum))),
        odd_even_buckets: tally(metrics.iter().map(|m| m.odd_even_text())),
        big_small_buckets: tally(metrics.iter().map(|m| m.big_small_text())),
        prime_buckets: tally(metrics.iter().map(|m| m.prime_text())),
        road_buckets: tally(metrics.iter().map(|m| m.road_text())),
        zone_buckets: tally(metrics.iter().map(|m| m.zone_text())),
        ac_buckets: tally(metrics.iter().map(|m| m.ac.to_string())),
        latest_hot_reds: hot_reds.clone(),
        latest_hot_blues: hot_blues.clone(),
        hot_reds,
        cold_reds,
        hot_blues,
        cold_blues,
        metrics,
        reds,
        blues,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(code: &str, reds: [u8; 6], blue: u8) -> Draw {
        Draw::new(code.to_string(), "2026-01-01".to_string(), reds, blue)
    }

    #[test]
    fn ac_of_known_draw() {
        // 差値去重后 13 个 -> AC = 13 - 5 = 8
        assert_eq!(ac_value(&[5, 6, 10, 25, 27, 30]), 8);
        // 01,02,03,04,05,06：差値 1..5 共 5 个 -> AC = 0
        assert_eq!(ac_value(&[1, 2, 3, 4, 5, 6]), 0);
    }

    #[test]
    fn metrics_basics() {
        let cur = d("2026002", [1, 2, 3, 11, 22, 33], 5);
        let prev = d("2026001", [2, 3, 12, 21, 22, 32], 6);
        let m = draw_metrics(&cur, Some(&prev));
        assert_eq!(m.sum, 72);
        assert_eq!(m.span, 32);
        assert_eq!(m.odd, 4);
        assert_eq!(m.repeat, 3); // 2, 3, 22
        assert_eq!(m.neighbor, 3); // 1(←2), 11(←12), 33(←32)
        assert_eq!(m.consecutive_runs, 1);
        assert_eq!(m.same_tail, 6); // 1/11、2/22、3/33 三组共 6 个
        assert_eq!(m.tail_sum, 1 + 2 + 3 + 1 + 2 + 3);
    }

    #[test]
    fn omission_counts() {
        let draws = vec![
            d("2026004", [1, 2, 3, 4, 5, 7], 1),
            d("2026003", [1, 2, 3, 4, 5, 6], 1),
            d("2026002", [1, 2, 3, 4, 5, 6], 1),
            d("2026001", [7, 8, 9, 10, 11, 12], 2),
        ];
        let stats = omission_stats(&draws, Zone::Red);
        let n1 = stats.iter().find(|s| s.number == 1).unwrap();
        assert_eq!(n1.current, 0);
        assert_eq!(n1.hits, 3);
        assert_eq!(n1.streak, 3);
        let n6 = stats.iter().find(|s| s.number == 6).unwrap();
        assert_eq!(n6.current, 1); // 最新一期没有 6，上一次在 1 期前
        let n7 = stats.iter().find(|s| s.number == 7).unwrap();
        assert_eq!(n7.current, 0);
        let n8 = stats.iter().find(|s| s.number == 8).unwrap();
        assert_eq!(n8.current, 3);
    }

    #[test]
    fn analyze_smoke() {
        let draws = vec![
            d("2026002", [1, 2, 3, 4, 5, 6], 1),
            d("2026001", [7, 8, 9, 10, 11, 12], 2),
        ];
        let report = analyze(&draws);
        assert_eq!(report.total, 2);
        assert_eq!(report.metrics.len(), 2);
        assert_eq!(report.sum_buckets.iter().map(|b| b.count).sum::<u32>(), 2);
    }
}
