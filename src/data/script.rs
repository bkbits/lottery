//! 用内嵌的 Boa JS 引擎执行数据源脚本。
//!
//! 脚本里可用的宿主接口：
//! - `http.get(url)`：同步 GET，返回响应文本
//! - `log(message)`：写日志，出错时会一并返回给界面
//!
//! 脚本必须 `return` 一个 JSON 字符串，形如
//! `[{"code":"2026128","date":"2026-11-08","red":"05,06,10,25,27,30","blue":"11"}]`。

use std::cell::RefCell;

use anyhow::{Context, Result, anyhow, bail};
use boa_engine::{
    Context as JsContext, JsArgs, JsNativeError, JsString, JsValue, NativeFunction, Source,
};
use serde::{Deserialize, Deserializer};

use super::http;
use crate::core::draw::{Draw, normalize_code, normalize_date, parse_blue, parse_reds};

type Fetcher = Box<dyn Fn(&str) -> std::result::Result<String, String>>;

thread_local! {
    static FETCHER: RefCell<Option<Fetcher>> = const { RefCell::new(None) };
    static LOGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

const PRELUDE: &str = r#"
var http = {
    get: function (url) { return __http_get(String(url)); }
};
"#;

fn js_http_get(
    _this: &JsValue,
    args: &[JsValue],
    ctx: &mut JsContext,
) -> boa_engine::JsResult<JsValue> {
    let url = args
        .get_or_undefined(0)
        .to_string(ctx)?
        .to_std_string_escaped();

    let outcome = FETCHER.with(|cell| {
        let borrow = cell.borrow();
        match borrow.as_ref() {
            Some(fetcher) => fetcher(&url),
            None => Err("HTTP 通道未初始化".to_string()),
        }
    });

    match outcome {
        Ok(text) => Ok(JsValue::from(JsString::from(text.as_str()))),
        Err(message) => Err(JsNativeError::error().with_message(message).into()),
    }
}

fn js_log(_this: &JsValue, args: &[JsValue], ctx: &mut JsContext) -> boa_engine::JsResult<JsValue> {
    let mut parts = Vec::with_capacity(args.len());
    for arg in args {
        parts.push(arg.to_string(ctx)?.to_std_string_escaped());
    }
    LOGS.with(|logs| logs.borrow_mut().push(parts.join(" ")));
    Ok(JsValue::undefined())
}

/// 安装默认 HTTP 通道（真实网络）。测试可以调用 [`set_fetcher`] 替换。
pub fn install_default_fetcher() {
    let already = FETCHER.with(|cell| cell.borrow().is_some());
    if !already {
        set_fetcher(|url| http::get(url).map(|r| r.text).map_err(|e| format!("{e:#}")));
    }
}

pub fn set_fetcher(f: impl Fn(&str) -> std::result::Result<String, String> + 'static) {
    FETCHER.with(|cell| *cell.borrow_mut() = Some(Box::new(f)));
}

fn take_logs() -> Vec<String> {
    LOGS.with(|logs| std::mem::take(&mut *logs.borrow_mut()))
}

fn format_logs(logs: &[String]) -> String {
    if logs.is_empty() {
        String::new()
    } else {
        format!("\n脚本日志：\n{}", logs.join("\n"))
    }
}

#[derive(Debug, Deserialize)]
struct RawDraw {
    code: String,
    #[serde(default)]
    date: String,
    red: String,
    blue: String,
    #[serde(default, deserialize_with = "flex_opt_i64")]
    sales: Option<i64>,
    #[serde(default, deserialize_with = "flex_opt_i64")]
    pool: Option<i64>,
}

/// 各数据源金额字段格式不统一（数字 / 带逗号的字符串 / 空串）。
fn flex_opt_i64<'de, D>(deserializer: D) -> std::result::Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::Number(n)) => n.as_i64(),
        Some(serde_json::Value::String(s)) => s.replace(',', "").trim().parse::<i64>().ok(),
        _ => None,
    })
}

/// 执行脚本并把返回值转成字符串（不做业务解析），便于排查脚本问题。
pub fn eval_raw(name: &str, script: &str) -> Result<String> {
    install_default_fetcher();
    let _ = take_logs();

    let mut ctx = JsContext::default();

    ctx.register_global_builtin_callable(
        JsString::from("__http_get"),
        1,
        NativeFunction::from_fn_ptr(js_http_get),
    )
    .map_err(|e| anyhow!("注册 http.get 失败: {e}"))?;
    ctx.register_global_builtin_callable(
        JsString::from("log"),
        1,
        NativeFunction::from_fn_ptr(js_log),
    )
    .map_err(|e| anyhow!("注册 log 失败: {e}"))?;

    ctx.eval(Source::from_bytes(PRELUDE))
        .map_err(|e| anyhow!("初始化脚本环境失败: {e}"))?;

    // 脚本体即表达式（推荐写成 IIFE），求值结果作为返回值。
    let value = ctx.eval(Source::from_bytes(script)).map_err(|e| {
        let logs = take_logs();
        anyhow!("{name} 脚本执行失败: {e}{}", format_logs(&logs))
    })?;

    kernel_string(name, &value, &mut ctx)
}

fn kernel_string(name: &str, value: &JsValue, ctx: &mut JsContext) -> Result<String> {
    Ok(value
        .to_string(ctx)
        .map_err(|e| anyhow!("{name} 脚本返回值无法转成字符串: {e}"))?
        .to_std_string_escaped())
}

/// 执行一个数据源脚本，返回解析后的开奖记录。
pub fn run_script(name: &str, script: &str) -> Result<Vec<Draw>> {
    let json = eval_raw(name, script)?;

    let raw: Vec<RawDraw> = serde_json::from_str(&json).with_context(|| {
        format!(
            "{name} 脚本返回的 JSON 解析失败（前 200 字符：{}）",
            preview(&json)
        )
    })?;

    let mut out = Vec::with_capacity(raw.len());
    let mut skipped = 0usize;
    for item in raw {
        let Some(code) = normalize_code(&item.code) else {
            skipped += 1;
            continue;
        };
        let Some(reds) = parse_reds(&item.red) else {
            skipped += 1;
            continue;
        };
        let Some(blue) = parse_blue(&item.blue) else {
            skipped += 1;
            continue;
        };
        out.push(Draw {
            code,
            date: normalize_date(&item.date),
            reds,
            blue,
            sales: item.sales,
            pool: item.pool,
        });
    }

    let logs = take_logs();
    if out.is_empty() {
        bail!(
            "{name} 脚本没有返回有效数据（跳过 {skipped} 条）{}",
            format_logs(&logs)
        );
    }

    Ok(out)
}

fn preview(text: &str) -> String {
    let cut: String = text.chars().take(200).collect();
    cut.replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_script_with_fake_http() {
        let json = r#"[{"code":"2026128","date":"2026-11-08(日)","red":"05,06,10,25,27,30","blue":"11","sales":"442016354"}]"#;
        set_fetcher(move |_url| Ok(json.to_string()));
        let draws = run_script(
            "fake",
            "(function () { log('hello'); return http.get('https://example.com/x'); })();",
        )
        .expect("script should run");
        assert_eq!(draws.len(), 1);
        assert_eq!(draws[0].code, "2026128");
        assert_eq!(draws[0].date, "2026-11-08");
        assert_eq!(draws[0].reds, [5, 6, 10, 25, 27, 30]);
        assert_eq!(draws[0].blue, 11);
        assert_eq!(draws[0].sales, Some(442016354));
    }

    #[test]
    fn reports_http_failure() {
        set_fetcher(|_url| Err("网络不可用".to_string()));
        let err = run_script(
            "fake",
            "(function () { return http.get('https://example.com/x'); })();",
        )
        .unwrap_err();
        let text = format!("{err:#}");
        assert!(text.contains("网络不可用"), "{text}");
    }
}
