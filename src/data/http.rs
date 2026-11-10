//! 最小 HTTP 客户端封装，供 JS 数据源脚本调用。

use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context, Result, bail};

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

pub struct HttpResponse {
    pub status: u16,
    pub text: String,
}

/// 从 URL 推出站点根地址，作为 Referer。
fn origin_of(url: &str) -> String {
    let mut parts = url.splitn(4, '/');
    let scheme = parts.next().unwrap_or("https:");
    let _ = parts.next();
    let host = parts.next().unwrap_or("");
    format!("{scheme}//{host}/")
}

/// 全局复用同一个客户端，保留连接池（500.com 回填时会有多次请求）。
fn client() -> Result<&'static reqwest::blocking::Client> {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }
    let built = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(40))
        .connect_timeout(Duration::from_secs(15))
        .user_agent(USER_AGENT)
        .build()
        .context("创建 HTTP 客户端失败")?;
    Ok(CLIENT.get_or_init(|| built))
}

/// 同步 GET。调用方负责把它放到工作线程里跑。
pub fn get(url: &str) -> Result<HttpResponse> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        bail!("只支持 http/https 地址: {url}");
    }

    let response = client()?
        .get(url)
        .header("Accept", "*/*")
        .header("Accept-Language", "zh-CN,zh;q=0.9")
        .header("Referer", origin_of(url))
        .send()
        .with_context(|| format!("请求失败: {url}"))?;

    let status = response.status().as_u16();
    let bytes = response
        .bytes()
        .with_context(|| format!("读取响应失败: {url}"))?;

    if status != 200 {
        bail!("HTTP {status}: {url}");
    }

    // 两个数据源都是 UTF-8；非法字节用替换字符兜底，不影响数字解析。
    let text = match String::from_utf8(bytes.to_vec()) {
        Ok(text) => text,
        Err(err) => String::from_utf8_lossy(err.as_bytes()).into_owned(),
    };

    Ok(HttpResponse { status, text })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_parsing() {
        assert_eq!(
            origin_of("https://datachart.500.com/ssq/history/newinc/history.php?start=1"),
            "https://datachart.500.com/"
        );
        assert_eq!(
            origin_of("https://www.cwl.gov.cn/cwl_admin/front/x"),
            "https://www.cwl.gov.cn/"
        );
    }
}
