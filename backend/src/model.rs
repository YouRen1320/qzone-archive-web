use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum JobPhase {
    AwaitingLogin,
    LoggedIn,
    Queued,
    Archiving,
    DownloadingMedia,
    Packaging,
    Ready,
    Paused,
    Cancelled,
    Failed,
    Interrupted,
}

impl JobPhase {
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            Self::Queued | Self::Archiving | Self::DownloadingMedia | Self::Packaging
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobStatus {
    pub job_id: String,
    pub phase: JobPhase,
    pub message: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub logged_in: bool,
    pub masked_uin: Option<String>,
    pub pages: u64,
    pub fetched: u64,
    pub saved: u64,
    pub media_total: u64,
    pub media_downloaded: u64,
    pub media_failed: u64,
    pub include_media: bool,
    pub download_ready: bool,
    pub downloaded_at: Option<i64>,
}

impl JobStatus {
    pub fn new(job_id: String, created_at: i64, expires_at: i64) -> Self {
        Self {
            job_id,
            phase: JobPhase::AwaitingLogin,
            message: "请先使用 QQ 扫码登录".into(),
            created_at,
            expires_at,
            logged_in: false,
            masked_uin: None,
            pages: 0,
            fetched: 0,
            saved: 0,
            media_total: 0,
            media_downloaded: 0,
            media_failed: 0,
            include_media: true,
            download_ready: false,
            downloaded_at: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistentJob {
    pub schema_version: u32,
    pub token_hash: String,
    pub status: JobStatus,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartArchiveRequest {
    #[serde(default = "default_include_media")]
    pub include_media: bool,
    #[serde(default = "default_page_delay_ms")]
    pub page_delay_ms: u64,
    pub max_pages: Option<u64>,
}

fn default_include_media() -> bool {
    true
}

fn default_page_delay_ms() -> u64 {
    3_000
}

impl Default for StartArchiveRequest {
    fn default() -> Self {
        Self {
            include_media: true,
            page_delay_ms: default_page_delay_ms(),
            max_pages: None,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QrResponse {
    pub qr_image: String,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginResponse {
    pub status: String,
    pub message: String,
    pub masked_uin: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
    pub active_jobs: usize,
    pub stored_jobs: usize,
    pub storage_available_bytes: u64,
}
