use std::{env, net::SocketAddr, path::PathBuf, time::Duration};

use crate::error::AppError;

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: SocketAddr,
    pub data_dir: PathBuf,
    pub frontend_dir: PathBuf,
    pub public_origin: String,
    pub secure_cookies: bool,
    pub idle_job_ttl: Duration,
    pub job_ttl: Duration,
    pub ready_job_ttl: Duration,
    pub post_download_ttl: Duration,
    pub no_progress_timeout: Duration,
    pub max_run_duration: Duration,
    pub max_jobs: usize,
    pub max_active_archives: usize,
    pub max_job_bytes: u64,
    pub min_free_bytes: u64,
}

impl Config {
    pub fn from_env() -> Result<Self, AppError> {
        let bind = env::var("QZONE_BIND")
            .unwrap_or_else(|_| "127.0.0.1:8091".into())
            .parse()
            .map_err(|_| AppError::configuration("QZONE_BIND 不是有效的监听地址"))?;
        let public_origin = env::var("QZONE_PUBLIC_ORIGIN")
            .unwrap_or_else(|_| "http://localhost:5173".into())
            .trim_end_matches('/')
            .to_owned();
        if !(public_origin.starts_with("https://") || public_origin.starts_with("http://")) {
            return Err(AppError::configuration(
                "QZONE_PUBLIC_ORIGIN 必须是完整的 http(s) 地址",
            ));
        }

        let config = Self {
            bind,
            data_dir: PathBuf::from(
                env::var("QZONE_DATA_DIR").unwrap_or_else(|_| "./data/jobs".into()),
            ),
            frontend_dir: PathBuf::from(
                env::var("QZONE_FRONTEND_DIR").unwrap_or_else(|_| "../frontend/dist".into()),
            ),
            public_origin,
            secure_cookies: parse_bool("QZONE_SECURE_COOKIES", true)?,
            idle_job_ttl: Duration::from_secs(parse_u64("QZONE_IDLE_JOB_TTL_SECONDS", 1_200)?),
            job_ttl: Duration::from_secs(parse_u64("QZONE_JOB_TTL_SECONDS", 21_600)?),
            ready_job_ttl: Duration::from_secs(parse_u64("QZONE_READY_JOB_TTL_SECONDS", 7_200)?),
            post_download_ttl: Duration::from_secs(parse_u64(
                "QZONE_POST_DOWNLOAD_TTL_SECONDS",
                600,
            )?),
            no_progress_timeout: Duration::from_secs(parse_u64(
                "QZONE_NO_PROGRESS_TIMEOUT_SECONDS",
                300,
            )?),
            max_run_duration: Duration::from_secs(parse_u64("QZONE_MAX_RUN_SECONDS", 3_600)?),
            max_jobs: parse_usize("QZONE_MAX_JOBS", 8)?,
            max_active_archives: parse_usize("QZONE_MAX_ACTIVE_ARCHIVES", 1)?,
            max_job_bytes: parse_u64("QZONE_MAX_JOB_BYTES", 5 * 1024 * 1024 * 1024)?,
            min_free_bytes: parse_u64("QZONE_MIN_FREE_BYTES", 5 * 1024 * 1024 * 1024)?,
        };
        config.validate()?;
        Ok(config)
    }

    #[cfg(test)]
    pub fn development(data_dir: PathBuf, frontend_dir: PathBuf) -> Self {
        Self {
            bind: "127.0.0.1:0".parse().expect("test bind address is valid"),
            data_dir,
            frontend_dir,
            public_origin: "http://localhost".into(),
            secure_cookies: false,
            idle_job_ttl: Duration::from_secs(60),
            job_ttl: Duration::from_secs(3_600),
            ready_job_ttl: Duration::from_secs(120),
            post_download_ttl: Duration::from_secs(60),
            no_progress_timeout: Duration::from_secs(5),
            max_run_duration: Duration::from_secs(60),
            max_jobs: 8,
            max_active_archives: 1,
            max_job_bytes: 512 * 1024 * 1024,
            min_free_bytes: 1,
        }
    }

    fn validate(&self) -> Result<(), AppError> {
        if self.max_jobs == 0 || self.max_jobs > 64 {
            return Err(AppError::configuration(
                "QZONE_MAX_JOBS 必须处于 1 到 64 之间",
            ));
        }
        if self.max_active_archives == 0 || self.max_active_archives > 4 {
            return Err(AppError::configuration(
                "QZONE_MAX_ACTIVE_ARCHIVES 必须处于 1 到 4 之间",
            ));
        }
        if self.job_ttl < Duration::from_secs(600) {
            return Err(AppError::configuration(
                "QZONE_JOB_TTL_SECONDS 不能少于 600 秒",
            ));
        }
        if self.idle_job_ttl < Duration::from_secs(300) {
            return Err(AppError::configuration(
                "QZONE_IDLE_JOB_TTL_SECONDS 不能少于 300 秒",
            ));
        }
        if self.ready_job_ttl < Duration::from_secs(600) {
            return Err(AppError::configuration(
                "QZONE_READY_JOB_TTL_SECONDS 不能少于 600 秒",
            ));
        }
        if self.max_run_duration < Duration::from_secs(600) {
            return Err(AppError::configuration(
                "QZONE_MAX_RUN_SECONDS 不能少于 600 秒",
            ));
        }
        if self.no_progress_timeout < Duration::from_secs(60)
            || self.no_progress_timeout > self.max_run_duration
        {
            return Err(AppError::configuration(
                "QZONE_NO_PROGRESS_TIMEOUT_SECONDS 必须处于 60 秒到单次运行上限之间",
            ));
        }
        if self.post_download_ttl > self.ready_job_ttl {
            return Err(AppError::configuration(
                "下载后保留时间不能超过完成后保留时间",
            ));
        }
        Ok(())
    }

    pub fn owner_cookie_ttl(&self) -> Duration {
        self.job_ttl + self.max_run_duration + self.ready_job_ttl
    }
}

fn parse_u64(name: &str, default: u64) -> Result<u64, AppError> {
    match env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|_| AppError::configuration(format!("{name} 必须是正整数"))),
        Err(_) => Ok(default),
    }
}

fn parse_usize(name: &str, default: usize) -> Result<usize, AppError> {
    parse_u64(name, default as u64).and_then(|value| {
        usize::try_from(value).map_err(|_| AppError::configuration(format!("{name} 超出系统范围")))
    })
}

fn parse_bool(name: &str, default: bool) -> Result<bool, AppError> {
    match env::var(name) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            _ => Err(AppError::configuration(format!(
                "{name} 必须是 true 或 false"
            ))),
        },
        Err(_) => Ok(default),
    }
}
