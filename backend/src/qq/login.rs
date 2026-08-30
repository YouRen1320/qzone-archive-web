// Portions of the QQ QR-login flow are adapted from QzoneArchive
// (https://github.com/Gaoshu705/QzoneArchive), licensed under GPL-3.0.

use std::{
    collections::HashMap,
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rand::{rngs::OsRng, RngCore};
use regex::Regex;
use reqwest::{
    header::{COOKIE, USER_AGENT},
    redirect::Policy,
    Client, Response,
};
use tokio::sync::Mutex;

const APP_ID: &str = "549000929";
const DAID: &str = "5";
const XLOGIN_URL: &str = "https://xui.ptlogin2.qq.com/cgi-bin/xlogin";
const S_URL: &str = "https://h5.qzone.qq.com/mqzone/index";
const PROXY_URL: &str = "";
const MOBILE_USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 (iPhone; CPU iPhone OS 18_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.5 Mobile/15E148 Safari/604.1",
    "Mozilla/5.0 (iPhone; CPU iPhone OS 17_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.6 Mobile/15E148 Safari/604.1",
    "Mozilla/5.0 (Linux; Android 15; Pixel 8 Build/AP3A.241105.007) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36",
    "Mozilla/5.0 (Linux; Android 14; SM-S9280 Build/UP1A.231005.007) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Mobile Safari/537.36",
    "Mozilla/5.0 (Linux; Android 14; 23127PN0CC Build/UKQ1.231003.002) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36",
    "Mozilla/5.0 (Linux; Android 14; V2309A Build/UP1A.231005.007) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Mobile Safari/537.36",
];
static USER_AGENT_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

#[derive(Default)]
struct LoginSession {
    ptqrtoken: i64,
    cookies: HashMap<String, String>,
    uin: Option<String>,
    g_tk: Option<i64>,
    user_agent: String,
    login_sig: String,
}

pub struct QqLogin {
    client: Client,
    session: Mutex<Option<LoginSession>>,
    last_user_agent: Mutex<Option<String>>,
}

#[derive(Clone)]
pub(crate) struct QzoneAuth {
    pub uin: String,
    pub g_tk: i64,
    pub cookie_header: String,
    pub user_agent: String,
}

#[derive(Debug)]
pub struct QrLoginStart {
    pub qr_image: String,
}

#[derive(Debug)]
pub struct LoginStatus {
    pub status: &'static str,
    pub message: String,
    pub masked_uin: Option<String>,
}

impl QqLogin {
    pub fn new() -> Result<Self, String> {
        let client = Client::builder()
            .redirect(Policy::none())
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(35))
            .build()
            .map_err(|error| format!("无法创建 QQ 登录客户端：{error}"))?;
        Ok(Self {
            client,
            session: Mutex::new(None),
            last_user_agent: Mutex::new(None),
        })
    }

    pub(crate) fn client(&self) -> Client {
        self.client.clone()
    }

    pub(crate) async fn auth(&self) -> Result<QzoneAuth, String> {
        let guard = self.session.lock().await;
        let session = guard.as_ref().ok_or("尚未登录 QQ 空间")?;
        let g_tk = session.g_tk.ok_or("登录会话缺少 g_tk")?;
        if session
            .cookies
            .get("p_skey")
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err("登录会话缺少有效的 p_skey".into());
        }
        Ok(QzoneAuth {
            uin: session.uin.clone().ok_or("登录会话缺少 uin")?,
            g_tk,
            cookie_header: cookie_header(&session.cookies),
            user_agent: session.user_agent.clone(),
        })
    }

    pub async fn is_logged_in(&self) -> bool {
        self.auth().await.is_ok()
    }

    pub async fn clear(&self) {
        *self.session.lock().await = None;
    }

    pub async fn start_qr_login(&self) -> Result<QrLoginStart, String> {
        let user_agent = self.next_mobile_user_agent().await;
        let (login_sig, mut cookies) = fetch_login_sig(&self.client, &user_agent).await?;
        let response = self
            .client
            .get("https://ssl.ptlogin2.qq.com/ptqrshow")
            .header(USER_AGENT, &user_agent)
            .header(COOKIE, cookie_header(&cookies))
            .query(&[
                ("appid", APP_ID),
                ("e", "2"),
                ("l", "M"),
                ("s", "3"),
                ("d", "72"),
                ("v", "4"),
                ("t", &unix_millis().to_string()),
                ("daid", DAID),
                ("pt_3rd_aid", "0"),
                ("u1", S_URL),
            ])
            .send()
            .await
            .map_err(|error| format!("获取登录二维码失败：{error}"))?;
        if !response.status().is_success() {
            return Err(format!("获取登录二维码失败：HTTP {}", response.status()));
        }
        let qrsig = response
            .cookies()
            .find(|cookie| cookie.name() == "qrsig")
            .map(|cookie| cookie.value().to_owned())
            .ok_or("二维码响应中缺少 qrsig")?;
        merge_response_cookies(&response, &mut cookies);
        add_browser_fingerprint_cookies(&mut cookies);
        let image = response
            .bytes()
            .await
            .map_err(|error| format!("读取二维码失败：{error}"))?;
        *self.session.lock().await = Some(LoginSession {
            ptqrtoken: ptqr_token(&qrsig),
            cookies,
            user_agent,
            login_sig,
            ..Default::default()
        });
        Ok(QrLoginStart {
            qr_image: format!("data:image/png;base64,{}", BASE64.encode(image)),
        })
    }

    pub async fn poll_qr_login(&self) -> Result<LoginStatus, String> {
        let mut guard = self.session.lock().await;
        let session = guard.as_mut().ok_or("请先获取登录二维码")?;
        let response = self
            .client
            .get("https://ssl.ptlogin2.qq.com/ptqrlogin")
            .header(USER_AGENT, &session.user_agent)
            .header(COOKIE, cookie_header(&session.cookies))
            .query(&[
                ("u1", S_URL),
                ("ptqrtoken", &session.ptqrtoken.to_string()),
                ("ptredirect", "0"),
                ("h", "1"),
                ("t", "1"),
                ("g", "1"),
                ("from_ui", "1"),
                ("ptlang", "2052"),
                ("action", &format!("0-0-{}", unix_millis())),
                ("js_ver", "20032614"),
                ("js_type", "1"),
                ("login_sig", &session.login_sig),
                ("pt_uistyle", "40"),
                ("has_onekey", "1"),
                ("o1vId", ""),
                ("aid", APP_ID),
                ("daid", DAID),
            ])
            .send()
            .await
            .map_err(|error| format!("检查扫码状态失败：{error}"))?;
        merge_response_cookies(&response, &mut session.cookies);
        let text = response
            .text()
            .await
            .map_err(|error| format!("读取扫码状态失败：{error}"))?;

        if text.contains("'66'") || text.contains("二维码未失效") {
            return Ok(login_status("waiting", "请使用手机 QQ 扫描二维码", None));
        }
        if text.contains("'67'") || text.contains("二维码认证中") {
            return Ok(login_status("scanned", "已扫码，请在手机 QQ 中确认", None));
        }
        if text.contains("'65'") || text.contains("二维码已失效") {
            return Ok(login_status("expired", "二维码已失效，请刷新后重试", None));
        }
        if !(text.contains("'0'") || text.contains("登录成功")) {
            return Ok(login_status("error", "QQ 登录返回了无法识别的状态", None));
        }

        let login_url = poll_login_url(&text).unwrap_or_else(|| {
            let ptsigx = callback_query_value(&text, "ptsigx").unwrap_or_default();
            let uin = callback_query_value(&text, "uin").unwrap_or_default();
            format!("https://ptlogin2.qzone.qq.com/check_sig?pttype=1&uin={uin}&service=ptqrlogin&nodirect=0&ptsigx={ptsigx}&s_url={S_URL}&f_url=&ptlang=2052&ptredirect=100&aid={APP_ID}&daid={DAID}")
        });
        let callback_uin = callback_query_value(&text, "uin").ok_or("登录成功响应中缺少 uin")?;
        let response = self
            .client
            .get(&login_url)
            .header(USER_AGENT, &session.user_agent)
            .header(COOKIE, cookie_header(&session.cookies))
            .send()
            .await
            .map_err(|error| format!("确认 QQ 登录失败：{error}"))?;
        merge_response_cookies(&response, &mut session.cookies);
        let uin = normalized_uin(&callback_uin);
        let p_skey = session
            .cookies
            .get("p_skey")
            .filter(|value| !value.trim().is_empty())
            .ok_or("登录 Cookie 中缺少有效的 p_skey")?;
        session.g_tk = Some(bkn(p_skey));
        session.uin = Some(uin.clone());
        session.user_agent = account_user_agent(&uin);
        let warmup_ua = session.user_agent.clone();
        warmup_qzone_session(&self.client, &mut session.cookies, &warmup_ua, &uin).await;
        Ok(login_status("success", "登录成功", Some(mask_uin(&uin))))
    }

    async fn next_mobile_user_agent(&self) -> String {
        let mut previous = self.last_user_agent.lock().await;
        let selected = select_mobile_user_agent(previous.as_deref());
        *previous = Some(selected.clone());
        selected
    }
}

fn login_status(
    status: &'static str,
    message: impl Into<String>,
    masked_uin: Option<String>,
) -> LoginStatus {
    LoginStatus {
        status,
        message: message.into(),
        masked_uin,
    }
}

fn mask_uin(uin: &str) -> String {
    let chars = uin.chars().collect::<Vec<_>>();
    if chars.len() <= 4 {
        return "****".into();
    }
    format!(
        "{}****{}",
        chars.iter().take(2).collect::<String>(),
        chars.iter().skip(chars.len() - 2).collect::<String>()
    )
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn secure_random_hex(len: usize) -> String {
    let mut bytes = vec![0_u8; len.div_ceil(2)];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes).chars().take(len).collect()
}

fn secure_random_alphanum(len: usize) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut bytes = vec![0_u8; len];
    OsRng.fill_bytes(&mut bytes);
    bytes
        .into_iter()
        .map(|value| CHARSET[value as usize % CHARSET.len()] as char)
        .collect()
}

fn select_mobile_user_agent(previous: Option<&str>) -> String {
    let sequence = USER_AGENT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let seed = unix_millis() as usize ^ sequence.wrapping_mul(0x9E37_79B1);
    let mut index = seed % MOBILE_USER_AGENTS.len();
    if previous.is_some_and(|value| value == MOBILE_USER_AGENTS[index]) {
        index = (index + 1) % MOBILE_USER_AGENTS.len();
    }
    MOBILE_USER_AGENTS[index].to_owned()
}

fn account_user_agent(uin: &str) -> String {
    let hash = uin.bytes().fold(0_u32, |acc, byte| {
        acc.wrapping_mul(31).wrapping_add(byte as u32)
    });
    MOBILE_USER_AGENTS[hash as usize % MOBILE_USER_AGENTS.len()].to_owned()
}

fn callback_query_value(text: &str, name: &str) -> Option<String> {
    let pattern = format!(r"(?:[?&]|'){name}=([^&']+)");
    Regex::new(&pattern)
        .ok()?
        .captures(text)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().to_owned())
}

fn poll_login_url(text: &str) -> Option<String> {
    let expression = Regex::new(r"'([^']*)'").ok()?;
    let values = expression
        .captures_iter(text)
        .filter_map(|capture| capture.get(1))
        .map(|value| value.as_str())
        .collect::<Vec<_>>();
    (values.len() >= 3 && values[0] == "0").then(|| values[2].to_owned())
}

fn ptqr_token(qrsig: &str) -> i64 {
    let mut value: u32 = 0;
    for character in qrsig.chars() {
        value = value
            .wrapping_add(value.wrapping_shl(5))
            .wrapping_add(character as u32);
    }
    (value & 0x7fff_ffff) as i64
}

fn bkn(p_skey: &str) -> i64 {
    let mut value: u32 = 5381;
    for character in p_skey.chars() {
        value = value
            .wrapping_add(value.wrapping_shl(5))
            .wrapping_add(character as u32);
    }
    (value & 0x7fff_ffff) as i64
}

fn normalized_uin(value: &str) -> String {
    value
        .trim_start_matches('o')
        .trim_start_matches('0')
        .to_owned()
}

fn cookie_header(cookies: &HashMap<String, String>) -> String {
    cookies
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

fn merge_response_cookies(response: &Response, cookies: &mut HashMap<String, String>) {
    for cookie in response.cookies() {
        let value = cookie.value().trim();
        if !value.is_empty() {
            cookies.insert(cookie.name().to_owned(), value.to_owned());
        }
    }
}

fn add_browser_fingerprint_cookies(cookies: &mut HashMap<String, String>) {
    cookies.insert("_qimei_fingerprint".into(), secure_random_hex(32));
    cookies.insert("_qimei_uuid42".into(), secure_random_hex(42));
    cookies.insert("_qimei_i_3".into(), secure_random_hex(87));
    cookies.insert(
        "_qimei_h38".into(),
        format!("{}0{}", secure_random_hex(25), secure_random_hex(12)),
    );
    cookies.insert("_qimei_i_1".into(), secure_random_hex(97));
    cookies.insert(
        "_qpsvr_localtk".into(),
        format!("{:.16}", unix_millis() as f64 / 1e18),
    );
    cookies
        .entry("RK".into())
        .or_insert_with(|| secure_random_alphanum(10));
    cookies
        .entry("ptcz".into())
        .or_insert_with(|| secure_random_hex(64));
    let timestamp = unix_millis();
    cookies
        .entry("pgv_pvid".into())
        .or_insert_with(|| format!("{}", timestamp % 9_000_000_000 + 1_000_000_000));
    cookies
        .entry("pgv_info".into())
        .or_insert_with(|| format!("ssid=s{timestamp}"));
    for (name, value) in [
        ("QZ_FE_WEBP_SUPPORT", "1"),
        ("cpu_performance_v8", "0"),
        ("__Q_w_s_hat_seed", "1"),
        ("domainid", "5"),
    ] {
        cookies.entry(name.into()).or_insert_with(|| value.into());
    }
    for name in ["fqm_pvqid", "fqm_sessionid"] {
        cookies.entry(name.into()).or_insert_with(|| {
            format!(
                "{}-{}-{}-{}-{}",
                secure_random_hex(8),
                secure_random_hex(4),
                secure_random_hex(4),
                secure_random_hex(4),
                secure_random_hex(12)
            )
        });
    }
}

async fn fetch_login_sig(
    client: &Client,
    user_agent: &str,
) -> Result<(String, HashMap<String, String>), String> {
    let response = client
        .get(XLOGIN_URL)
        .header(USER_AGENT, user_agent)
        .query(&[
            ("hide_title_bar", "1"),
            ("style", "22"),
            ("daid", DAID),
            ("low_login", "0"),
            ("qlogin_auto_login", "1"),
            ("no_verifyimg", "1"),
            ("link_target", "blank"),
            ("appid", APP_ID),
            ("target", "self"),
            ("s_url", S_URL),
            ("proxy_url", PROXY_URL),
            ("pt_no_auth", "1"),
        ])
        .send()
        .await
        .map_err(|error| format!("xlogin 请求失败：{error}"))?;
    if !response.status().is_success() {
        return Err(format!("xlogin 返回 HTTP {}", response.status()));
    }
    let mut cookies = HashMap::new();
    merge_response_cookies(&response, &mut cookies);
    let sig = cookies
        .remove("pt_login_sig")
        .ok_or("xlogin 响应中缺少 pt_login_sig cookie")?;
    Ok((sig, cookies))
}

async fn warmup_qzone_session(
    client: &Client,
    cookies: &mut HashMap<String, String>,
    user_agent: &str,
    uin: &str,
) {
    if let Ok(response) = client
        .get(S_URL)
        .header(USER_AGENT, user_agent)
        .header(COOKIE, cookie_header(cookies))
        .send()
        .await
    {
        if response.status().is_success() || response.status().is_redirection() {
            merge_response_cookies(&response, cookies);
        }
    }
    cookies
        .entry("ptui_loginuin".into())
        .or_insert_with(|| uin.to_owned());
    for (name, value) in [
        ("QZ_FE_WEBP_SUPPORT", "1"),
        ("cpu_performance_v8", "0"),
        ("__Q_w_s_hat_seed", "1"),
        ("domainid", "5"),
    ] {
        cookies.entry(name.into()).or_insert_with(|| value.into());
    }
}

#[cfg(test)]
mod tests {
    use super::{bkn, callback_query_value, mask_uin, ptqr_token};

    #[test]
    fn login_hashes_match_reference_algorithm() {
        assert_eq!(ptqr_token("abc"), 108_966);
        assert_eq!(bkn("abc"), 193_485_963);
    }

    #[test]
    fn extracts_callback_values_without_exposing_other_text() {
        let response = "ptuiCB('0','0','https://ptlogin2.qzone.qq.com/check_sig?uin=o01941163264&ptsigx=abc123&service=ptqrlogin','0','登录成功！','昵称');";
        assert_eq!(
            callback_query_value(response, "uin").as_deref(),
            Some("o01941163264")
        );
        assert_eq!(mask_uin("1941163264"), "19****64");
    }
}
