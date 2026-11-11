//! UI 绑定：把数据层、统计、预测接到 Slint 界面上。

use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use crate::core::predict::{AlgoInfo, BLUE_ALGOS, RED_ALGOS, RULES};
use crate::core::stats::{Bucket, OmissionStat, StatsReport, analyze};
use crate::core::{BlueAlgo, Draw, PredictConfig, Prediction, RedAlgo, Rule};
use crate::data::{Store, fetch_history};

use crate::{AlgoRow, AppWindow, BucketRow, HistoryRow, OmissionRow, PredRow, StatRow};

const CONFIG_KEY: &str = "predict-config";
const HISTORY_PAGE_SIZE: usize = 80;

struct AppState {
    store: Option<Arc<Store>>,
    draws: Vec<Draw>,
    report: StatsReport,
    cfg: PredictConfig,
    predictions: Vec<Prediction>,
    busy: bool,
    status: String,
    detail: String,
}

impl AppState {
    fn new(store: Option<Arc<Store>>) -> Self {
        Self {
            store,
            draws: Vec::new(),
            report: StatsReport::default(),
            cfg: PredictConfig::default(),
            predictions: Vec::new(),
            busy: false,
            status: "尚未加载".to_string(),
            detail: "正在准备数据库…".to_string(),
        }
    }

    fn load_from_store(&mut self) {
        let Some(store) = self.store.as_ref() else {
            self.status = "数据库不可用".to_string();
            self.detail = "仅能使用内存数据".to_string();
            return;
        };

        match store.load_draws() {
            Ok(draws) => {
                self.draws = draws;
                self.report = analyze(&self.draws);
                if let Ok(Some(text)) = store.get_setting(CONFIG_KEY)
                    && let Ok(cfg) = serde_json::from_str::<PredictConfig>(&text)
                {
                    self.cfg = cfg;
                }
                self.refresh_status();
            }
            Err(err) => {
                self.status = "读取失败".to_string();
                self.detail = format!("{err:#}");
            }
        }
    }

    fn refresh_status(&mut self) {
        if self.draws.is_empty() {
            self.status = "无数据".to_string();
            self.detail = "点击右上角「刷新数据」抓取历史开奖".to_string();
            return;
        }
        self.status = if self.busy {
            "抓取中".to_string()
        } else {
            format!("已载入 {} 期", self.draws.len())
        };
        let latest = &self.draws[0];
        let oldest_code = self
            .draws
            .last()
            .map(|d| d.code.clone())
            .unwrap_or_default();
        self.detail = format!(
            "最新 {} 期 · {} · 红 {} 蓝 {}　|　历史 {} 至 {}，共 {} 期",
            latest.code,
            latest.date,
            latest.red_text(" "),
            latest.blue_text(),
            oldest_code,
            latest.code,
            self.draws.len()
        );
    }

    fn persist_config(&self) {
        let Some(store) = self.store.as_ref() else {
            return;
        };
        if let Ok(text) = serde_json::to_string(&self.cfg) {
            let _ = store.set_setting(CONFIG_KEY, &text);
        }
    }
}

type Shared = Arc<Mutex<AppState>>;

fn lock(state: &Shared) -> MutexGuard<'_, AppState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn run() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    slint_kit::install(&ui);

    let (store, store_error) = match Store::open_default() {
        Ok(store) => (Some(Arc::new(store)), None),
        Err(err) => (None, Some(format!("{err:#}"))),
    };

    let mut state = AppState::new(store.clone());
    state.load_from_store();
    if let Some(err) = &store_error {
        state.status = "数据库不可用".to_string();
        state.detail = err.clone();
    }

    let state: Shared = Arc::new(Mutex::new(state));
    wire(&ui, state.clone());

    {
        let st = lock(&state);
        render(&ui, &st);
    }

    // 首次启动、库里没有数据时自动抓取一次。
    if lock(&state).draws.is_empty() && store.is_some() {
        let weak = ui.as_weak();
        slint::Timer::single_shot(std::time::Duration::from_millis(80), move || {
            if let Some(ui) = weak.upgrade() {
                ui.invoke_refresh();
            }
        });
    }

    ui.run()
}

fn wire(ui: &AppWindow, state: Shared) {
    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_history_load_more(move || {
            if let Some(ui) = weak.upgrade() {
                let rows = ui.get_history_rows();
                if let Some(model) = rows.as_any().downcast_ref::<VecModel<HistoryRow>>() {
                    append_history_rows(&lock(&state), model);
                }
            }
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_refresh(move || {
            if let Some(ui) = weak.upgrade() {
                start_fetch(&ui, state.clone());
            }
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_generate(move || {
            if let Some(ui) = weak.upgrade() {
                generate_predictions(&ui, &state);
            }
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_toggle_algo(move |group, id| {
            if let Some(ui) = weak.upgrade() {
                toggle_algo(&ui, &state, group, id);
            }
        });
    }

    {
        let weak = ui.as_weak();
        ui.on_show_info(move |group, id| {
            if let Some(ui) = weak.upgrade() {
                let (title, body) = algo_info(group, id);
                ui.set_info_title(title.into());
                ui.set_info_body(body.into());
                ui.set_info_open(true);
            }
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_copy_all(move || {
            if let Some(ui) = weak.upgrade() {
                let lines: Vec<String> = lock(&state)
                    .predictions
                    .iter()
                    .map(Prediction::line)
                    .collect();
                if lines.is_empty() {
                    show_alert(&ui, 2, "还没有预测结果，先生成再复制。");
                    return;
                }
                match copy_to_clipboard(&lines.join("\n")) {
                    Ok(()) => {
                        show_alert(&ui, 1, format!("已复制 {} 组号码到剪贴板。", lines.len()))
                    }
                    Err(err) => show_alert(&ui, 3, format!("复制失败：{err:#}")),
                }
            }
        });
    }

    {
        let weak = ui.as_weak();
        ui.on_copy_line(move |line: SharedString| {
            if let Some(ui) = weak.upgrade() {
                match copy_to_clipboard(line.as_str()) {
                    Ok(()) => show_alert(&ui, 1, format!("已复制：{line}")),
                    Err(err) => show_alert(&ui, 3, format!("复制失败：{err:#}")),
                }
            }
        });
    }
}

fn start_fetch(ui: &AppWindow, state: Shared) {
    let store = {
        let st = lock(&state);
        if st.busy {
            return;
        }
        let Some(store) = st.store.clone() else {
            drop(st);
            show_alert(ui, 3, "数据库不可用，无法保存数据。");
            return;
        };
        store
    };

    {
        let mut st = lock(&state);
        st.busy = true;
        st.status = "抓取中".to_string();
        st.detail = "正在从福彩官网与 500.com 抓取历史数据…".to_string();
    }
    ui.set_busy(true);
    ui.set_status_text("抓取中".into());
    ui.set_status_detail("正在从福彩官网与 500.com 抓取历史数据…".into());

    let weak = ui.as_weak();
    thread::spawn(move || {
        let outcome = fetch_history();
        let mut message = outcome.message_text();

        let stored = store.insert_new(&outcome.draws);
        let draws = match store.load_draws() {
            Ok(draws) if !draws.is_empty() => draws,
            _ => outcome.draws.clone(),
        };

        let failed = stored.is_err() || message.contains("失败（");

        match stored {
            Ok(count) => message = format!("{message}；新增 {count} 期"),
            Err(err) => message = format!("{message}；写入数据库失败（{err:#}）"),
        }

        let state_for_ui = state.clone();
        let weak_for_ui = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak_for_ui.upgrade() {
                let mut st = lock(&state_for_ui);
                st.draws = draws;
                st.report = analyze(&st.draws);
                st.predictions.clear();
                st.busy = false;
                st.refresh_status();

                ui.set_busy(false);
                render(&ui, &st);
                drop(st);

                show_alert(&ui, if failed { 2 } else { 1 }, message);
            }
        });
    });
}

fn generate_predictions(ui: &AppWindow, state: &Shared) {
    let count: usize = ui.get_predict_count().trim().parse().unwrap_or(5);

    let mut st = lock(state);
    if st.draws.is_empty() {
        drop(st);
        show_alert(ui, 2, "还没有历史数据，先刷新一次。");
        return;
    }

    st.cfg.count = count.clamp(1, 20);
    let predictions = crate::core::predict::generate(&st.draws, &st.report, &st.cfg);
    st.predictions = predictions;
    st.persist_config();
    let total = st.predictions.len();

    ui.set_predict_count(st.cfg.count.to_string().into());
    render_predictions(ui, &st);
    drop(st);

    if total == 0 {
        show_alert(ui, 2, "当前规则组合过严，没有生成结果，试试放宽过滤规则。");
    }
}

fn toggle_algo(ui: &AppWindow, state: &Shared, group: i32, id: i32) {
    let mut st = lock(state);
    match group {
        0 => st.cfg.red = red_algo_by_id(id),
        1 => st.cfg.blue = blue_algo_by_id(id),
        _ => {
            let rule = rule_by_id(id);
            if let Some(pos) = st.cfg.rules.iter().position(|r| *r == rule) {
                st.cfg.rules.remove(pos);
            } else {
                st.cfg.rules.push(rule);
            }
        }
    }
    st.persist_config();
    render_algorithms(ui, &st);
}

fn algo_info(group: i32, id: i32) -> (String, String) {
    let table: &[AlgoInfo] = match group {
        0 => &RED_ALGOS,
        1 => &BLUE_ALGOS,
        _ => &RULES,
    };
    match table.iter().find(|a| a.id == id) {
        Some(info) => (
            format!("{} · {}", info.name, info.summary),
            info.detail.to_string(),
        ),
        None => (
            "未知算法".to_string(),
            "没有找到对应的算法说明。".to_string(),
        ),
    }
}

fn red_algo_by_id(id: i32) -> RedAlgo {
    match id {
        1 => RedAlgo::Hot,
        2 => RedAlgo::Omission,
        3 => RedAlgo::Markov,
        4 => RedAlgo::MeanRegression,
        _ => RedAlgo::Balanced,
    }
}

fn blue_algo_by_id(id: i32) -> BlueAlgo {
    match id {
        1 => BlueAlgo::Hot,
        2 => BlueAlgo::Random,
        _ => BlueAlgo::Omission,
    }
}

fn rule_by_id(id: i32) -> Rule {
    match id {
        1 => Rule::SumWindow,
        2 => Rule::OddEvenBalanced,
        3 => Rule::AtLeastOneConsecutive,
        _ => Rule::ExcludeRepeat,
    }
}

fn red_algo_id(algo: RedAlgo) -> i32 {
    match algo {
        RedAlgo::Balanced => 0,
        RedAlgo::Hot => 1,
        RedAlgo::Omission => 2,
        RedAlgo::Markov => 3,
        RedAlgo::MeanRegression => 4,
    }
}

fn blue_algo_id(algo: BlueAlgo) -> i32 {
    match algo {
        BlueAlgo::Omission => 0,
        BlueAlgo::Hot => 1,
        BlueAlgo::Random => 2,
    }
}

fn rule_id(rule: Rule) -> i32 {
    match rule {
        Rule::ExcludeRepeat => 0,
        Rule::SumWindow => 1,
        Rule::OddEvenBalanced => 2,
        Rule::AtLeastOneConsecutive => 3,
    }
}

fn show_alert(ui: &AppWindow, level: i32, message: impl Into<SharedString>) {
    ui.set_alert_level(level);
    ui.set_alert_text(message.into());
    ui.set_alert_open(true);
    ui.set_alert_token(ui.get_alert_token().wrapping_add(1));
}

// ------------------------------------------------------------------ 渲染

fn render(ui: &AppWindow, state: &AppState) {
    ui.set_status_text(state.status.clone().into());
    ui.set_status_detail(state.detail.clone().into());
    ui.set_busy(state.busy);
    ui.set_predict_count(state.cfg.count.to_string().into());

    render_algorithms(ui, state);
    render_predictions(ui, state);
    render_overview(ui, state);
    render_omissions(ui, state);
    render_buckets(ui, state);
    render_features(ui, state);
    render_history(ui, state);
}

fn render_algorithms(ui: &AppWindow, state: &AppState) {
    let reds: Vec<AlgoRow> = RED_ALGOS
        .iter()
        .map(|info| AlgoRow {
            id: info.id,
            group: 0,
            name: info.name.into(),
            summary: info.summary.into(),
            selected: red_algo_id(state.cfg.red) == info.id,
        })
        .collect();

    let blues: Vec<AlgoRow> = BLUE_ALGOS
        .iter()
        .map(|info| AlgoRow {
            id: info.id,
            group: 1,
            name: info.name.into(),
            summary: info.summary.into(),
            selected: blue_algo_id(state.cfg.blue) == info.id,
        })
        .collect();

    let rules: Vec<AlgoRow> = RULES
        .iter()
        .map(|info| AlgoRow {
            id: info.id,
            group: 2,
            name: info.name.into(),
            summary: info.summary.into(),
            selected: state.cfg.rules.iter().any(|r| rule_id(*r) == info.id),
        })
        .collect();

    ui.set_red_algos(ModelRc::new(VecModel::from(reds)));
    ui.set_blue_algos(ModelRc::new(VecModel::from(blues)));
    ui.set_rules(ModelRc::new(VecModel::from(rules)));
}

fn render_predictions(ui: &AppWindow, state: &AppState) {
    let rows: Vec<PredRow> = state
        .predictions
        .iter()
        .enumerate()
        .map(|(index, p)| {
            let reds: Vec<SharedString> = p.reds.iter().map(|n| format!("{n:02}").into()).collect();
            let odd = p.odd_count();
            PredRow {
                no: format!("{:02}", index + 1).into(),
                reds: ModelRc::new(VecModel::from(reds)),
                blue: p.blue_text().into(),
                line: p.line().into(),
                meta: format!("和值 {} · 奇偶 {odd}:{}", p.sum(), 6 - odd).into(),
            }
        })
        .collect();
    ui.set_predictions(ModelRc::new(VecModel::from(rows)));
}

fn stat(label: &str, value: String, hint: &str) -> StatRow {
    StatRow {
        label: label.into(),
        value: value.into(),
        hint: hint.into(),
    }
}

fn render_overview(ui: &AppWindow, state: &AppState) {
    let report = &state.report;
    let stats = if report.total == 0 {
        vec![
            stat("历史期数", "0".to_string(), "等待抓取"),
            stat("最新期号", "-".to_string(), "-"),
            stat("平均和值", "-".to_string(), "-"),
            stat("平均跨度", "-".to_string(), "-"),
        ]
    } else {
        vec![
            stat("历史期数", report.total.to_string(), "已入库的开奖期数"),
            stat(
                "最新期号",
                report.latest_code.clone(),
                &format!("{} 开奖", report.latest_date),
            ),
            stat(
                "平均和值",
                format!("{:.1}", report.sum_avg),
                &format!("区间 {} - {}", report.sum_min, report.sum_max),
            ),
            stat(
                "平均跨度",
                format!("{:.1}", report.span_avg),
                "红球最大最小差",
            ),
            stat(
                "含连号占比",
                format!("{:.1}%", report.consecutive_rate),
                "至少一组连号",
            ),
            stat(
                "平均重号",
                format!("{:.2}", report.repeat_avg),
                "与上期重复个数",
            ),
        ]
    };
    ui.set_overview_stats(ModelRc::new(VecModel::from(stats)));
}

fn omission_rows(
    stats: &[OmissionStat],
    hot: &[u8],
    cold: &[u8],
    range: std::ops::RangeInclusive<u8>,
) -> Vec<OmissionRow> {
    stats
        .iter()
        .filter(|s| range.contains(&s.number))
        .map(|s| OmissionRow {
            cells: ModelRc::new(VecModel::from(vec![
                SharedString::from(format!("{:02}", s.number)),
                SharedString::from(s.current.to_string()),
                SharedString::from(format!("{:.1}", s.average)),
                SharedString::from(s.max.to_string()),
                SharedString::from(s.streak.to_string()),
            ])),
            tone: if hot.contains(&s.number) {
                1
            } else if cold.contains(&s.number) {
                2
            } else {
                0
            },
        })
        .collect()
}

fn rows_of(rows: Vec<OmissionRow>) -> ModelRc<OmissionRow> {
    ModelRc::new(VecModel::from(rows))
}

fn strings_of(numbers: &[u8]) -> ModelRc<SharedString> {
    let items: Vec<SharedString> = numbers.iter().map(|n| format!("{n:02}").into()).collect();
    ModelRc::new(VecModel::from(items))
}

fn render_omissions(ui: &AppWindow, state: &AppState) {
    let report = &state.report;
    if report.reds.is_empty() {
        ui.set_omission_red_a(rows_of(Vec::new()));
        ui.set_omission_red_b(rows_of(Vec::new()));
        ui.set_omission_red_c(rows_of(Vec::new()));
        ui.set_omission_blue_a(rows_of(Vec::new()));
        ui.set_omission_blue_b(rows_of(Vec::new()));
        ui.set_hot_reds(strings_of(&[]));
        ui.set_cold_reds(strings_of(&[]));
        ui.set_hot_blues(strings_of(&[]));
        ui.set_cold_blues(strings_of(&[]));
        return;
    }

    ui.set_omission_red_a(rows_of(omission_rows(
        &report.reds,
        &report.hot_reds,
        &report.cold_reds,
        1..=11,
    )));
    ui.set_omission_red_b(rows_of(omission_rows(
        &report.reds,
        &report.hot_reds,
        &report.cold_reds,
        12..=22,
    )));
    ui.set_omission_red_c(rows_of(omission_rows(
        &report.reds,
        &report.hot_reds,
        &report.cold_reds,
        23..=33,
    )));
    ui.set_omission_blue_a(rows_of(omission_rows(
        &report.blues,
        &report.hot_blues,
        &report.cold_blues,
        1..=8,
    )));
    ui.set_omission_blue_b(rows_of(omission_rows(
        &report.blues,
        &report.hot_blues,
        &report.cold_blues,
        9..=16,
    )));

    ui.set_hot_reds(strings_of(&report.hot_reds));
    ui.set_cold_reds(strings_of(&report.cold_reds));
    ui.set_hot_blues(strings_of(&report.hot_blues));
    ui.set_cold_blues(strings_of(&report.cold_blues));
}

fn bucket_rows(buckets: &[Bucket]) -> ModelRc<BucketRow> {
    let max = buckets.iter().map(|b| b.count).max().unwrap_or(1).max(1);
    let rows: Vec<BucketRow> = buckets
        .iter()
        .map(|b| BucketRow {
            label: b.label.clone().into(),
            count: format!("{} 次", b.count).into(),
            percent: format!("{:.1}%", b.percent).into(),
            ratio: (b.count as f32 / max as f32).clamp(0.02, 1.0),
        })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

fn render_buckets(ui: &AppWindow, state: &AppState) {
    let report = &state.report;
    ui.set_sum_buckets(bucket_rows(&report.sum_buckets));
    ui.set_odd_even_buckets(bucket_rows(&report.odd_even_buckets));
    ui.set_big_small_buckets(bucket_rows(&report.big_small_buckets));
    ui.set_prime_buckets(bucket_rows(&report.prime_buckets));
    ui.set_road_buckets(bucket_rows(&report.road_buckets));
    ui.set_zone_buckets(bucket_rows(&report.zone_buckets));
    ui.set_ac_buckets(bucket_rows(&report.ac_buckets));
}

fn render_features(ui: &AppWindow, state: &AppState) {
    let report = &state.report;
    let rows = if report.total == 0 {
        Vec::new()
    } else {
        vec![
            stat("平均和值", format!("{:.1}", report.sum_avg), "全部历史期次"),
            stat(
                "平均跨度",
                format!("{:.1}", report.span_avg),
                "红球最大减最小",
            ),
            stat("平均 AC 值", format!("{:.2}", report.ac_avg), "差值多样性"),
            stat(
                "平均尾数和",
                format!("{:.1}", report.tail_sum_avg),
                "6 个红球个位之和",
            ),
            stat(
                "平均重号",
                format!("{:.2}", report.repeat_avg),
                "与上期重复个数",
            ),
            stat(
                "平均邻号",
                format!("{:.2}", report.neighbor_avg),
                "上期号码 ±1",
            ),
            stat(
                "平均同尾",
                format!("{:.2}", report.same_tail_avg),
                "同尾号个数",
            ),
            stat(
                "含连号期数占比",
                format!("{:.1}%", report.consecutive_rate),
                "至少一组相邻号码",
            ),
            stat(
                "数据范围",
                format!("{} - {}", report.oldest_code, report.latest_code),
                &format!("{} 至 {}", report.oldest_date, report.latest_date),
            ),
        ]
    };
    ui.set_feature_stats(ModelRc::new(VecModel::from(rows)));
}

fn history_rows(state: &AppState, start: usize, count: usize) -> Vec<HistoryRow> {
    state
        .draws
        .iter()
        .enumerate()
        .skip(start)
        .take(count)
        .map(|(index, draw)| {
            let metrics = state.report.metrics.get(index);
            let cell = |value: String| SharedString::from(value);
            let cells: Vec<SharedString> = vec![
                cell(draw.code.clone()),
                cell(draw.date.clone()),
                cell(draw.red_text(" ")),
                cell(draw.blue_text()),
                cell(metrics.map(|m| m.sum.to_string()).unwrap_or_default()),
                cell(metrics.map(|m| m.span.to_string()).unwrap_or_default()),
                cell(metrics.map(|m| m.odd_even_text()).unwrap_or_default()),
                cell(metrics.map(|m| m.big_small_text()).unwrap_or_default()),
                cell(metrics.map(|m| m.prime_text()).unwrap_or_default()),
                cell(metrics.map(|m| m.road_text()).unwrap_or_default()),
                cell(metrics.map(|m| m.zone_text()).unwrap_or_default()),
                cell(metrics.map(|m| m.ac.to_string()).unwrap_or_default()),
                cell(metrics.map(|m| m.consecutive_text()).unwrap_or_default()),
                cell(metrics.map(|m| m.repeat.to_string()).unwrap_or_default()),
                cell(metrics.map(|m| m.same_tail_text()).unwrap_or_default()),
                cell(metrics.map(|m| m.neighbor.to_string()).unwrap_or_default()),
                cell(metrics.map(|m| m.tail_sum.to_string()).unwrap_or_default()),
            ];
            HistoryRow {
                cells: ModelRc::new(VecModel::from(cells)),
            }
        })
        .collect()
}

fn append_history_rows(state: &AppState, model: &VecModel<HistoryRow>) {
    for row in history_rows(state, model.row_count(), HISTORY_PAGE_SIZE) {
        model.push(row);
    }
}

fn render_history(ui: &AppWindow, state: &AppState) {
    ui.set_history_total(state.draws.len().min(i32::MAX as usize) as i32);
    ui.set_history_rows(ModelRc::new(VecModel::from(history_rows(
        state,
        0,
        HISTORY_PAGE_SIZE,
    ))));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_batches_preserve_order_and_metrics() {
        let mut state = AppState::new(None);
        state.draws = (0..HISTORY_PAGE_SIZE * 4 + 7)
            .map(|index| {
                Draw::new(
                    format!("2026{index:03}"),
                    "2026-01-01".to_string(),
                    [1, 2, 3, 4, 5, 6 + (index % 28) as u8],
                    1,
                )
            })
            .collect();
        state.report = analyze(&state.draws);
        let model = VecModel::from(history_rows(&state, 0, HISTORY_PAGE_SIZE));
        assert_eq!(model.row_count(), HISTORY_PAGE_SIZE);

        for expected_count in [160, 240, 320, 327, 327, 327] {
            append_history_rows(&state, &model);
            assert_eq!(model.row_count(), expected_count);
            for index in 0..expected_count {
                let row = model.row_data(index).unwrap();
                assert_eq!(
                    row.cells.row_data(0).unwrap().as_str(),
                    state.draws[index].code.as_str()
                );
                assert_eq!(
                    row.cells.row_data(4).unwrap().as_str(),
                    state.report.metrics[index].sum.to_string()
                );
            }
        }
        assert!(history_rows(&state, state.draws.len(), HISTORY_PAGE_SIZE).is_empty());
        assert!(history_rows(&state, usize::MAX, usize::MAX).is_empty());
        assert!(history_rows(&state, 0, 0).is_empty());
    }

    #[test]
    fn history_batches_handle_empty_data() {
        let state = AppState::new(None);
        let model = VecModel::from(history_rows(&state, 0, HISTORY_PAGE_SIZE));
        append_history_rows(&state, &model);
        append_history_rows(&state, &model);
        assert_eq!(model.row_count(), 0);
        assert!(history_rows(&state, usize::MAX, HISTORY_PAGE_SIZE).is_empty());
    }
}

#[cfg(not(target_os = "android"))]
fn copy_to_clipboard(text: &str) -> anyhow::Result<()> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| anyhow::anyhow!("{e}"))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}

#[cfg(target_os = "android")]
fn copy_to_clipboard(text: &str) -> anyhow::Result<()> {
    i_slint_backend_selector::with_platform(|platform| {
        platform.set_clipboard_text(text, slint::platform::Clipboard::DefaultClipboard);
        Ok(())
    })
    .map_err(|err| anyhow::anyhow!("{err}"))
}
