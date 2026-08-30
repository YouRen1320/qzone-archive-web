// The endpoint shape and browser headers are adapted from QzoneArchive
// (https://github.com/Gaoshu705/QzoneArchive), licensed under GPL-3.0.

use reqwest::header::{
    ACCEPT, ACCEPT_LANGUAGE, CACHE_CONTROL, COOKIE, ORIGIN, PRAGMA, REFERER, USER_AGENT,
};
use serde_json::Value;

use super::login::QqLogin;

const FEEDS_URL: &str = "https://mobile.qzone.qq.com/get_feeds";
const RESPONSE_ATTEMPTS: u32 = 6;

#[derive(Debug)]
pub struct FeedPage {
    pub feeds: Vec<Value>,
    pub attach_info: Option<String>,
    pub has_more: bool,
}

pub async fn fetch_feeds(
    login: &QqLogin,
    refresh_type: &str,
    attach_info: Option<&str>,
) -> Result<FeedPage, String> {
    let auth = login.auth().await?;
    let mut query = vec![
        ("g_tk", auth.g_tk.to_string()),
        ("res_type", "1".into()),
        ("refresh_type", refresh_type.into()),
        ("format", "json".into()),
    ];
    if let Some(cursor) = attach_info {
        if cursor.trim().is_empty() {
            return Err("分页游标不能为空".into());
        }
        query.push(("res_attach", cursor.to_owned()));
    }

    let client = login.client();
    let mut final_reason = "QQ 空间接口暂时不可用".to_owned();
    for attempt in 1..=RESPONSE_ATTEMPTS {
        let response = client
            .get(FEEDS_URL)
            .header(ACCEPT, "application/json")
            .header(
                ACCEPT_LANGUAGE,
                "zh-CN,zh;q=0.9,en;q=0.8,en-GB;q=0.7,en-US;q=0.6",
            )
            .header(CACHE_CONTROL, "no-cache")
            .header(PRAGMA, "no-cache")
            .header(ORIGIN, "https://h5.qzone.qq.com")
            .header(REFERER, "https://h5.qzone.qq.com/")
            .header(USER_AGENT, &auth.user_agent)
            .header(COOKIE, &auth.cookie_header)
            .header("Sec-Fetch-Dest", "empty")
            .header("Sec-Fetch-Mode", "cors")
            .header("Sec-Fetch-Site", "same-site")
            .header("Sec-Ch-Ua", sec_ch_ua(&auth.user_agent))
            .header("Sec-Ch-Ua-Mobile", "?1")
            .header("Sec-Ch-Ua-Platform", sec_platform(&auth.user_agent))
            .query(&query)
            .send()
            .await;

        match response {
            Ok(response) => {
                let status = response.status();
                let body = match response.text().await {
                    Ok(body) => body,
                    Err(error) => {
                        final_reason = if error.is_timeout() {
                            "读取 QQ 空间响应超时".into()
                        } else {
                            "QQ 空间响应不完整".into()
                        };
                        retry_delay(attempt).await;
                        continue;
                    }
                };
                if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                    final_reason = format!("QQ 空间接口返回 HTTP {status}");
                    retry_delay(attempt).await;
                    continue;
                }
                if !status.is_success() {
                    return Err(format!("QQ 空间接口拒绝请求：HTTP {status}"));
                }
                let value = serde_json::from_str::<Value>(&body).map_err(|_| {
                    "QQ 空间返回了无法解析的数据；已保留当前归档进度，请稍后重试".to_owned()
                })?;
                match parse_feed_page(value) {
                    Ok(page) => return Ok(page),
                    Err(error) if retryable_api_error(&error) => {
                        final_reason = error;
                        retry_delay(attempt).await;
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(error) => {
                final_reason = if error.is_timeout() {
                    "连接 QQ 空间超时".into()
                } else if error.is_connect() {
                    "暂时无法连接 QQ 空间".into()
                } else {
                    "QQ 空间请求传输失败".into()
                };
                retry_delay(attempt).await;
            }
        }
    }
    Err(format!("{final_reason}（已重试 {RESPONSE_ATTEMPTS} 次）"))
}

fn parse_feed_page(value: Value) -> Result<FeedPage, String> {
    if let Some(code) = value.get("code").and_then(Value::as_i64) {
        if code != 0 {
            let message = value
                .get("message")
                .or_else(|| value.get("msg"))
                .and_then(Value::as_str)
                .unwrap_or("未知错误");
            return Err(format!("QQ 空间动态接口返回错误 {code}：{message}"));
        }
    }
    let data = value.get("data").ok_or("QQ 空间响应中暂时缺少 data")?;
    let feeds = data
        .get("vFeeds")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let attach_info = data
        .get("attachinfo")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let server_has_more = data.get("hasmore").and_then(Value::as_i64).unwrap_or(0) != 0;
    let has_more = server_has_more && !feeds.is_empty() && attach_info.is_some();
    Ok(FeedPage {
        feeds,
        attach_info,
        has_more,
    })
}

fn retryable_api_error(error: &str) -> bool {
    if error.contains("未登录")
        || error.contains("登录失效")
        || error.contains("权限")
        || error.contains("封禁")
        || error.contains("p_skey")
    {
        return false;
    }
    error.contains("暂时") || error.starts_with("QQ 空间动态接口返回错误")
}

async fn retry_delay(attempt: u32) {
    if attempt < RESPONSE_ATTEMPTS {
        tokio::time::sleep(std::time::Duration::from_millis(
            1_500 * 2_u64.pow(attempt.saturating_sub(1)),
        ))
        .await;
    }
}

fn sec_ch_ua(user_agent: &str) -> String {
    if let Some(start) = user_agent.find("Chrome/") {
        let version_start = start + 7;
        let major = user_agent[version_start..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>();
        let version = if major.is_empty() { "131" } else { &major };
        format!(
            "\"Not;A=Brand\";v=\"8\", \"Chromium\";v=\"{version}\", \"Microsoft Edge\";v=\"{version}\""
        )
    } else {
        "\"Not;A=Brand\";v=\"8\", \"Apple\";v=\"0\", \"Safari\";v=\"18\"".into()
    }
}

fn sec_platform(user_agent: &str) -> &'static str {
    if user_agent.contains("iPhone") {
        "\"iOS\""
    } else {
        "\"Android\""
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::parse_feed_page;

    #[test]
    fn parses_first_page_and_cursor() {
        let page = parse_feed_page(json!({
            "code": 0,
            "data": { "attachinfo": "next", "hasmore": 1, "vFeeds": [{"id": 1}] }
        }))
        .unwrap();
        assert_eq!(page.feeds.len(), 1);
        assert_eq!(page.attach_info.as_deref(), Some("next"));
        assert!(page.has_more);
    }

    #[test]
    fn empty_page_finishes_safely() {
        let page = parse_feed_page(json!({"code": 0, "data": {"vFeeds": []}})).unwrap();
        assert!(!page.has_more);
        assert!(page.feeds.is_empty());
    }
}
