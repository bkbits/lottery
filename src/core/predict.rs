//! 号码预测算法。
//!
//! 设计：红球策略与蓝球策略各自成组、组内互斥；过滤规则可叠加多选。
//! 因此"可以选多个算法"而不会产生互相矛盾的组合。

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use super::draw::{Draw, RED_COUNT, RED_MAX};
use super::stats::{StatsReport, Zone, omission_stats};

/// 红球策略（组内互斥，必选其一）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RedAlgo {
    /// 均衡随机
    Balanced,
    /// 热号追踪
    Hot,
    /// 遗漏回补
    Omission,
    /// 转移概率（马尔可夫）
    Markov,
    /// 均值回归
    MeanRegression,
}

/// 蓝球策略（组内互斥，必选其一）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlueAlgo {
    /// 遗漏优先
    Omission,
    /// 热号优先
    Hot,
    /// 均匀随机
    Random,
}

/// 可叠加的过滤规则。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rule {
    /// 排除上期红球
    ExcludeRepeat,
    /// 和值 80~130
    SumWindow,
    /// 奇偶 3:3
    OddEvenBalanced,
    /// 至少一组连号
    AtLeastOneConsecutive,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PredictConfig {
    pub count: usize,
    pub red: RedAlgo,
    pub blue: BlueAlgo,
    pub rules: Vec<Rule>,
}

impl Default for PredictConfig {
    fn default() -> Self {
        Self {
            count: 5,
            red: RedAlgo::Balanced,
            blue: BlueAlgo::Omission,
            rules: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prediction {
    pub reds: [u8; RED_COUNT],
    pub blue: u8,
}

impl Prediction {
    pub fn red_text(&self, sep: &str) -> String {
        self.reds
            .iter()
            .map(|n| format!("{n:02}"))
            .collect::<Vec<_>>()
            .join(sep)
    }

    pub fn blue_text(&self) -> String {
        format!("{:02}", self.blue)
    }

    /// 复制用的单行格式：`01,02,03,04,05,06+08`
    pub fn line(&self) -> String {
        format!("{}+{}", self.red_text(","), self.blue_text())
    }

    pub fn odd_count(&self) -> usize {
        self.reds.iter().filter(|n| **n % 2 == 1).count()
    }

    pub fn sum(&self) -> u32 {
        self.reds.iter().map(|n| *n as u32).sum()
    }
}

/// 算法说明，供 UI 展示（标题 + 详细说明）。
pub struct AlgoInfo {
    pub id: i32,
    pub name: &'static str,
    pub summary: &'static str,
    pub detail: &'static str,
}

pub const RED_ALGOS: [AlgoInfo; 5] = [
    AlgoInfo {
        id: 0,
        name: "均衡随机",
        summary: "均匀抽样 + 形态约束",
        detail: "在 33 个红球中等概率抽取 6 个，只保留满足奇偶比 2:4~4:2、大小比 2:4~4:2、和值 60~170 的组合。\n\n优点：不依赖历史偏好，避免极端组合。\n适合作为基准对照组，用来衡量其它策略的偏离程度。",
    },
    AlgoInfo {
        id: 1,
        name: "热号追踪",
        summary: "按近 30 期频次加权",
        detail: "统计每个红球在最近 30 期的出现次数，用 次数^1.6 作为抽样权重，出现越多被选中的概率越高。\n\n思路：短期热度存在惯性。\n注意：热号也可能随时转冷，建议与过滤规则配合使用。",
    },
    AlgoInfo {
        id: 2,
        name: "遗漏回补",
        summary: "偏向超期未出的号码",
        detail: "权重 = (当前遗漏 + 1) / (平均遗漏 + 1)，再取平方强化差异。\n\n思路：号码出现频率长期趋于均匀，超过平均遗漏越多的号码，回补期望越高。\n这是经典的\"追冷\"策略。",
    },
    AlgoInfo {
        id: 3,
        name: "转移概率",
        summary: "用上期号码做条件概率",
        detail: "统计历史上两类条件概率：\n• P(本期出现 | 上期出现)\n• P(本期出现 | 上期未出现)\n\n再按提升度加权：上期开过的号用第一类概率，没开过的用第二类。\n\n思路：重号与邻号现象带来的短期相关性。",
    },
    AlgoInfo {
        id: 4,
        name: "均值回归",
        summary: "遗漏接近平均值的号码",
        detail: "为每个号码计算 |当前遗漏 - 平均遗漏|，偏离越小权重越高。\n\n思路：号码进入稳定周期时，遗漏值会在均值附近波动，据此挑选「节奏正常」的号码。\n与「遗漏回补」相反，它不追超期号。",
    },
];

pub const BLUE_ALGOS: [AlgoInfo; 3] = [
    AlgoInfo {
        id: 0,
        name: "遗漏优先",
        summary: "从最长遗漏中挑蓝球",
        detail: "取当前遗漏最大的 4 个蓝球，随机选 1 个。\n\n蓝球只有 16 个号码，平均 16 期出现一次，遗漏信号相对红球更稳定。",
    },
    AlgoInfo {
        id: 1,
        name: "热号优先",
        summary: "按近 30 期蓝球频次加权",
        detail: "统计每个蓝球最近 30 期出现次数，按 次数 + 0.5 加权抽样。\n\n适合跟随蓝球的短期热度。",
    },
    AlgoInfo {
        id: 2,
        name: "均匀随机",
        summary: "16 个蓝球等概率",
        detail: "每个蓝球被选中的概率相同（1/16）。\n\n当红球策略已经很强时，蓝球用随机可以避免过度拟合。",
    },
];

pub const RULES: [AlgoInfo; 4] = [
    AlgoInfo {
        id: 0,
        name: "排除上期红球",
        summary: "不选上期开出的号码",
        detail: "预测红球中不允许出现上期开出的 6 个红球。\n\n历史上单期完全重号极少，排除上期号码可降低无效组合。",
    },
    AlgoInfo {
        id: 1,
        name: "和值 80~130",
        summary: "限定和值区间",
        detail: "只接受红球和值落在 80 到 130 之间的组合。\n\n双色球红球和值理论范围 21~183，绝大多数期次集中在 80~130。",
    },
    AlgoInfo {
        id: 2,
        name: "奇偶 3:3",
        summary: "强制 3 奇 3 偶",
        detail: "只接受 3 个奇数 + 3 个偶数的组合。\n\n奇偶 3:3 是历史出现比例最高的形态之一。",
    },
    AlgoInfo {
        id: 3,
        name: "至少一组连号",
        summary: "要求存在相邻号码",
        detail: "要求 6 个红球中至少存在一对相邻号码（如 12、13）。\n\n连号在历史期次中出现比例较高，可用于筛选\"更像真实开奖\"的组合。",
    },
];

/// 简单可复现的 xorshift 随机数发生器，避免额外依赖。
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn from_clock() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        Self::new(nanos ^ (std::process::id() as u64).rotate_left(17))
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }
}

/// 加权无放回抽样（Efraimidis-Spirakis），返回 0 基下标。
fn weighted_sample(rng: &mut Rng, weights: &[f64], take: usize) -> Vec<usize> {
    let mut keyed: Vec<(f64, usize)> = weights
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let w = w.max(1e-6);
            (rng.unit().powf(1.0 / w), i)
        })
        .collect();
    keyed.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    keyed.into_iter().take(take).map(|(_, i)| i).collect()
}

fn to_reds(indices: Vec<usize>) -> [u8; RED_COUNT] {
    let mut reds: Vec<u8> = indices.into_iter().map(|i| (i + 1) as u8).collect();
    reds.sort_unstable();
    reds.dedup();
    let mut out = [1u8; RED_COUNT];
    for (slot, value) in out.iter_mut().zip(reds) {
        *slot = value;
    }
    out
}

fn has_consecutive(reds: &[u8; RED_COUNT]) -> bool {
    reds.windows(2).any(|w| w[1] == w[0] + 1)
}

fn passes_rules(rules: &[Rule], reds: &[u8; RED_COUNT], last: Option<&Draw>) -> bool {
    for rule in rules {
        match rule {
            Rule::ExcludeRepeat => {
                if let Some(last) = last
                    && reds.iter().any(|n| last.reds.contains(n))
                {
                    return false;
                }
            }
            Rule::SumWindow => {
                let sum: u32 = reds.iter().map(|n| *n as u32).sum();
                if !(80..=130).contains(&sum) {
                    return false;
                }
            }
            Rule::OddEvenBalanced => {
                let odd = reds.iter().filter(|n| **n % 2 == 1).count();
                if odd != 3 {
                    return false;
                }
            }
            Rule::AtLeastOneConsecutive => {
                if !has_consecutive(reds) {
                    return false;
                }
            }
        }
    }
    true
}

/// 形态兜底：避免全奇/全偶、全大/全小、和值极端。
fn plausible(reds: &[u8; RED_COUNT], strict: bool) -> bool {
    let odd = reds.iter().filter(|n| **n % 2 == 1).count();
    let big = reds.iter().filter(|n| **n >= 17).count();
    let sum: u32 = reds.iter().map(|n| *n as u32).sum();
    if strict {
        (2..=4).contains(&odd) && (2..=4).contains(&big) && (60..=170).contains(&sum)
    } else {
        (1..=5).contains(&odd) && (1..=5).contains(&big)
    }
}

fn red_weights(algo: RedAlgo, report: &StatsReport, draws: &[Draw]) -> Vec<f64> {
    match algo {
        RedAlgo::Balanced => vec![1.0; RED_MAX as usize],
        RedAlgo::Hot => report
            .reds
            .iter()
            .map(|s| (s.recent_hits as f64 + 0.5).powf(1.6))
            .collect(),
        RedAlgo::Omission => report
            .reds
            .iter()
            .map(|s| {
                let ratio = (s.current as f64 + 1.0) / (s.average + 1.0);
                (ratio * ratio).max(0.01)
            })
            .collect(),
        RedAlgo::Markov => markov_weights(draws),
        RedAlgo::MeanRegression => report
            .reds
            .iter()
            .map(|s| {
                let deviation = (s.current as f64 - s.average).abs();
                1.0 / (1.0 + deviation)
            })
            .collect(),
    }
}

/// 转移概率：按"上期是否出现"分别统计条件出现率，再以提升度加权。
fn markov_weights(draws: &[Draw]) -> Vec<f64> {
    let n = RED_MAX as usize;
    let mut hit_with_prev = vec![0.0f64; n];
    let mut total_with_prev = vec![0.0f64; n];
    let mut hit_all = vec![0.0f64; n];
    let mut total_all = 0.0f64;

    for pair in draws.windows(2) {
        let (cur, prev) = (&pair[0], &pair[1]);
        total_all += 1.0;
        for idx in 0..n {
            let number = (idx + 1) as u8;
            let appeared = cur.reds.contains(&number);
            if appeared {
                hit_all[idx] += 1.0;
            }
            if prev.reds.contains(&number) {
                total_with_prev[idx] += 1.0;
                if appeared {
                    hit_with_prev[idx] += 1.0;
                }
            }
        }
    }

    let last = draws.first();
    (0..n)
        .map(|idx| {
            let number = (idx + 1) as u8;
            let base = (hit_all[idx] + 1.0) / (total_all + 2.0);
            let appeared_last = last.is_some_and(|d| d.reds.contains(&number));
            let score = if appeared_last {
                // 重号条件概率相对基准的提升
                let p = (hit_with_prev[idx] + 1.0) / (total_with_prev[idx] + 2.0);
                (p * p / base).max(0.01)
            } else {
                base
            };
            score
        })
        .collect()
}

fn pick_blue(algo: BlueAlgo, report: &StatsReport, draws: &[Draw], rng: &mut Rng) -> u8 {
    let blues = if report.blues.len() == BLUE_DEFAULT {
        report.blues.clone()
    } else {
        omission_stats(draws, Zone::Blue)
    };
    match algo {
        BlueAlgo::Omission => {
            let mut sorted = blues;
            sorted.sort_by(|a, b| b.current.cmp(&a.current));
            let pool: Vec<u8> = sorted.iter().take(4).map(|s| s.number).collect();
            pool[rng.below(pool.len())]
        }
        BlueAlgo::Hot => {
            let weights: Vec<f64> = blues.iter().map(|s| s.recent_hits as f64 + 0.5).collect();
            let idx = weighted_sample(rng, &weights, 1);
            (idx[0] + 1) as u8
        }
        BlueAlgo::Random => (rng.below(16) + 1) as u8,
    }
}

const BLUE_DEFAULT: usize = 16;

/// 生成预测号码。
pub fn generate(draws: &[Draw], report: &StatsReport, cfg: &PredictConfig) -> Vec<Prediction> {
    if draws.is_empty() {
        return Vec::new();
    }
    let count = cfg.count.clamp(1, 20);
    let weights = red_weights(cfg.red, report, draws);
    let last = draws.first();
    let mut rng = Rng::from_clock();
    let mut out: Vec<Prediction> = Vec::with_capacity(count);

    for _ in 0..count {
        let mut chosen: Option<[u8; RED_COUNT]> = None;
        // 前 800 次按"严格形态 + 全部规则"筛选，之后逐级放宽，保证一定能出号。
        for attempt in 0..2000 {
            let strict = attempt < 800;
            let enforce_rules = attempt < 1600;
            let idx = weighted_sample(&mut rng, &weights, RED_COUNT);
            let reds = to_reds(idx);
            if !plausible(&reds, strict) {
                continue;
            }
            if enforce_rules && !passes_rules(&cfg.rules, &reds, last) {
                continue;
            }
            if out.iter().any(|p| p.reds == reds) {
                continue;
            }
            chosen = Some(reds);
            break;
        }
        let Some(reds) = chosen else { continue };
        let blue = pick_blue(cfg.blue, report, draws, &mut rng);
        out.push(Prediction { reds, blue });
    }

    out
}

/// 把期号字符串格式化成分组展示用文本。
pub fn format_line(p: &Prediction) -> String {
    p.line()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_draws() -> Vec<Draw> {
        let mut draws = Vec::new();
        let mut seed = 7u64;
        for i in 0..120 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let mut reds = [0u8; 6];
            let mut filled = 0usize;
            while filled < reds.len() {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let n = (seed >> 33) as u8 % 33 + 1;
                if !reds[..filled].contains(&n) {
                    reds[filled] = n;
                    filled += 1;
                }
            }
            reds.sort_unstable();
            draws.push(Draw::new(
                format!("2026{:03}", 120 - i),
                "2026-01-01".to_string(),
                reds,
                (i % 16 + 1) as u8,
            ));
        }
        draws
    }

    #[test]
    fn generates_unique_predictions() {
        let draws = sample_draws();
        let report = crate::core::stats::analyze(&draws);
        let cfg = PredictConfig {
            count: 10,
            red: RedAlgo::Balanced,
            blue: BlueAlgo::Random,
            rules: vec![Rule::OddEvenBalanced],
        };
        let out = generate(&draws, &report, &cfg);
        assert_eq!(out.len(), 10);
        for p in &out {
            assert_eq!(p.odd_count(), 3, "规则应被满足: {}", p.line());
            assert!(p.reds.iter().all(|n| (1..=33).contains(n)));
        }
        let mut lines: Vec<String> = out.iter().map(|p| p.line()).collect();
        lines.sort();
        lines.dedup();
        assert_eq!(lines.len(), 10);
    }

    #[test]
    fn every_red_algo_works() {
        let draws = sample_draws();
        let report = crate::core::stats::analyze(&draws);
        for red in [
            RedAlgo::Balanced,
            RedAlgo::Hot,
            RedAlgo::Omission,
            RedAlgo::Markov,
            RedAlgo::MeanRegression,
        ] {
            let cfg = PredictConfig {
                count: 3,
                red,
                blue: BlueAlgo::Omission,
                rules: vec![],
            };
            assert_eq!(generate(&draws, &report, &cfg).len(), 3, "{red:?}");
        }
    }

    #[test]
    fn all_rules_combined() {
        let draws = sample_draws();
        let report = crate::core::stats::analyze(&draws);
        let cfg = PredictConfig {
            count: 5,
            red: RedAlgo::Omission,
            blue: BlueAlgo::Hot,
            rules: vec![
                Rule::ExcludeRepeat,
                Rule::SumWindow,
                Rule::OddEvenBalanced,
                Rule::AtLeastOneConsecutive,
            ],
        };
        let out = generate(&draws, &report, &cfg);
        assert_eq!(out.len(), 5);
        for p in &out {
            assert_eq!(p.odd_count(), 3);
            assert!((80..=130).contains(&p.sum()));
            assert!(has_consecutive(&p.reds));
        }
    }
}
