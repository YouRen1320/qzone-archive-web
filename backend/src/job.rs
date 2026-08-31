use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::{
    sync::{broadcast, Mutex, Notify, OwnedSemaphorePermit, RwLock, Semaphore},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::{
    config::Config,
    error::AppError,
    model::{JobPhase, JobStatus, PersistentJob, StartArchiveRequest},
    qq::QqLogin,
    tokens::{new_job_id, new_owner_token, token_hash, token_matches, valid_job_id},
};

pub struct JobRuntime {
    pub id: String,
    pub dir: PathBuf,
    pub login: QqLogin,
    token_hash: String,
    status: RwLock<JobStatus>,
    pub(crate) lifecycle_lock: Mutex<()>,
    persist_lock: Mutex<()>,
    updates: broadcast::Sender<JobStatus>,
    cancellation: Mutex<CancellationToken>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl JobRuntime {
    fn new(
        id: String,
        dir: PathBuf,
        token_hash: String,
        status: JobStatus,
    ) -> Result<Self, AppError> {
        let (updates, _) = broadcast::channel(32);
        Ok(Self {
            id,
            dir,
            login: QqLogin::new().map_err(AppError::internal)?,
            token_hash,
            status: RwLock::new(status),
            lifecycle_lock: Mutex::new(()),
            persist_lock: Mutex::new(()),
            updates,
            cancellation: Mutex::new(CancellationToken::new()),
            task: Mutex::new(None),
        })
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        id: String,
        dir: PathBuf,
        token_hash: String,
        status: JobStatus,
    ) -> Result<Self, AppError> {
        Self::new(id, dir, token_hash, status)
    }

    pub fn db_path(&self) -> PathBuf {
        self.dir.join("archive.sqlite3")
    }

    pub fn media_dir(&self) -> PathBuf {
        self.dir.join("media")
    }

    pub fn export_dir(&self) -> PathBuf {
        self.dir.join("export")
    }

    pub fn download_path(&self) -> PathBuf {
        self.export_dir().join("qzone-archive.zip")
    }

    pub fn viewer_manifest_path(&self) -> PathBuf {
        self.export_dir().join("viewer-manifest.json")
    }

    pub async fn status(&self) -> JobStatus {
        self.status.read().await.clone()
    }

    pub(crate) fn status_blocking(&self) -> JobStatus {
        self.status.blocking_read().clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<JobStatus> {
        self.updates.subscribe()
    }

    pub async fn update(&self, change: impl FnOnce(&mut JobStatus)) -> Result<JobStatus, AppError> {
        let next = {
            let mut status = self.status.write().await;
            change(&mut status);
            status.clone()
        };
        self.persist().await?;
        let _ = self.updates.send(next.clone());
        Ok(next)
    }

    pub async fn persist(&self) -> Result<(), AppError> {
        let _guard = self.persist_lock.lock().await;
        let persistent = PersistentJob {
            schema_version: 1,
            token_hash: self.token_hash.clone(),
            status: self.status.read().await.clone(),
        };
        let encoded = serde_json::to_vec_pretty(&persistent)
            .map_err(|_| AppError::internal("无法序列化任务状态"))?;
        let temporary = self.dir.join("status.json.tmp");
        tokio::fs::write(&temporary, encoded).await?;
        set_private_file_permissions(&temporary).await?;
        tokio::fs::rename(&temporary, self.dir.join("status.json")).await?;
        Ok(())
    }

    pub async fn fresh_cancellation(&self) -> CancellationToken {
        let mut cancellation = self.cancellation.lock().await;
        cancellation.cancel();
        *cancellation = CancellationToken::new();
        cancellation.clone()
    }

    pub async fn cancel(&self) {
        self.cancellation.lock().await.cancel();
    }

    async fn replace_task(&self, task: JoinHandle<()>) {
        let mut current = self.task.lock().await;
        if let Some(previous) = current.take() {
            if !previous.is_finished() {
                previous.abort();
            }
        }
        *current = Some(task);
    }

    async fn abort_task(&self) {
        self.cancel().await;
        if let Some(task) = self.task.lock().await.take() {
            task.abort();
            let _ = task.await;
        }
    }
}

pub struct JobManager {
    pub config: Config,
    jobs: RwLock<HashMap<String, Arc<JobRuntime>>>,
    create_lock: Mutex<()>,
    queue: Mutex<VecDeque<String>>,
    queue_notify: Notify,
    archive_slots: Arc<Semaphore>,
    active_jobs: AtomicUsize,
}

impl JobManager {
    pub async fn new(config: Config) -> Result<Arc<Self>, AppError> {
        tokio::fs::create_dir_all(&config.data_dir).await?;
        let manager = Arc::new(Self {
            archive_slots: Arc::new(Semaphore::new(config.max_active_archives)),
            config,
            jobs: RwLock::new(HashMap::new()),
            create_lock: Mutex::new(()),
            queue: Mutex::new(VecDeque::new()),
            queue_notify: Notify::new(),
            active_jobs: AtomicUsize::new(0),
        });
        manager.load_existing().await?;
        Ok(manager)
    }

    pub async fn create(&self) -> Result<(Arc<JobRuntime>, String), AppError> {
        // Capacity check and directory allocation are one reservation boundary.
        let _create_guard = self.create_lock.lock().await;
        if self.jobs.read().await.len() >= self.config.max_jobs {
            return Err(AppError::unavailable(
                "job_capacity_reached",
                "当前等待任务已满，请稍后再试",
            ));
        }
        self.ensure_storage_available()?;

        let (id, dir) = loop {
            let id = new_job_id();
            let dir = self.config.data_dir.join(&id);
            if !dir.exists() {
                break (id, dir);
            }
        };
        tokio::fs::create_dir(&dir).await?;
        tokio::fs::create_dir(dir.join("media")).await?;
        tokio::fs::create_dir(dir.join("export")).await?;
        set_private_directory_permissions(&dir).await?;
        set_private_directory_permissions(&dir.join("media")).await?;
        set_private_directory_permissions(&dir.join("export")).await?;
        let owner_token = new_owner_token();
        let created_at = now();
        let status = JobStatus::new(id.clone(), created_at, self.idle_expires_at(created_at));
        let job = Arc::new(JobRuntime::new(
            id.clone(),
            dir,
            token_hash(&owner_token),
            status,
        )?);
        if let Err(error) = job.persist().await {
            let _ = tokio::fs::remove_dir_all(&job.dir).await;
            return Err(error);
        }
        self.jobs.write().await.insert(id, job.clone());
        Ok((job, owner_token))
    }

    pub async fn authorize(
        &self,
        id: &str,
        owner_token: &str,
    ) -> Result<Arc<JobRuntime>, AppError> {
        if !valid_job_id(id) || owner_token.len() != 64 {
            return Err(AppError::unauthorized());
        }
        let job = self
            .jobs
            .read()
            .await
            .get(id)
            .cloned()
            .ok_or_else(AppError::unauthorized)?;
        if !token_matches(owner_token, &job.token_hash) || job.status().await.expires_at <= now() {
            return Err(AppError::unauthorized());
        }
        Ok(job)
    }

    pub async fn start_archive(
        self: &Arc<Self>,
        job: Arc<JobRuntime>,
        request: StartArchiveRequest,
    ) -> Result<JobStatus, AppError> {
        // Serialize start/delete transitions for this job so only one task can be spawned.
        let _lifecycle_guard = job.lifecycle_lock.lock().await;
        let still_registered = self
            .jobs
            .read()
            .await
            .get(&job.id)
            .is_some_and(|current| Arc::ptr_eq(current, &job));
        if !still_registered {
            return Err(AppError::unauthorized());
        }
        if !job.login.is_logged_in().await {
            return Err(AppError::conflict(
                "login_required",
                "QQ 登录已经失效，请重新扫码",
            ));
        }
        if job.status().await.phase.is_active() {
            return Err(AppError::conflict(
                "job_already_running",
                "这个任务已经在运行",
            ));
        }
        self.ensure_storage_available()?;
        let request = StartArchiveRequest {
            page_delay_ms: request.page_delay_ms.clamp(2_000, 30_000),
            max_pages: request
                .max_pages
                .filter(|value| *value > 0)
                .map(|value| value.min(5_000)),
            ..request
        };
        let cancellation = job.fresh_cancellation().await;
        job.update(|status| {
            status.phase = JobPhase::Queued;
            status.message = "任务已进入安全队列，轮到后会自动开始".into();
            status.include_media = request.include_media;
            status.download_ready = false;
            status.last_activity_at = now();
            status.run_started_at = None;
            status.last_progress_at = None;
            status.expires_at = now() + self.config.job_ttl.as_secs() as i64;
        })
        .await?;
        {
            let mut queue = self.queue.lock().await;
            queue.push_back(job.id.clone());
        }
        self.refresh_queue_positions().await;
        let queued = job.status().await;

        let manager = self.clone();
        let task_job = job.clone();
        let task = tokio::spawn(async move {
            let permit = manager
                .acquire_archive_slot(&task_job.id, &cancellation)
                .await;
            let Some(permit) = permit else {
                manager.refresh_queue_positions().await;
                let _ = task_job
                    .update(|status| {
                        status.phase = JobPhase::Cancelled;
                        status.message = "任务已取消".into();
                        status.queued_ahead = 0;
                        status.expires_at = manager.ready_expires_at(now());
                    })
                    .await;
                return;
            };
            manager.active_jobs.fetch_add(1, Ordering::Relaxed);
            manager.refresh_queue_positions().await;
            let active_guard = ActiveJobGuard(&manager.active_jobs);
            let started_at = now();
            let _ = task_job
                .update(|status| {
                    status.queued_ahead = 0;
                    status.run_started_at = Some(started_at);
                    status.last_progress_at = Some(started_at);
                    status.last_activity_at = started_at;
                    status.expires_at = started_at
                        + manager.config.max_run_duration.as_secs() as i64
                        + manager.config.ready_job_ttl.as_secs() as i64;
                })
                .await;

            let archive = crate::archive::run(
                task_job.clone(),
                manager.config.clone(),
                request,
                cancellation.clone(),
            );
            tokio::pin!(archive);
            let mut watchdog = tokio::time::interval(std::time::Duration::from_secs(15));
            watchdog.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let mut automatic_pause = None;
            let result = loop {
                tokio::select! {
                    result = &mut archive => break result,
                    _ = watchdog.tick() => {
                        let status = task_job.status().await;
                        if let Some(reason) = watchdog_reason(&status, now(), &manager.config) {
                            automatic_pause = Some(reason);
                            cancellation.cancel();
                            break (&mut archive).await;
                        }
                    }
                }
            };
            match (automatic_pause, result) {
                (Some(reason), _) => {
                    task_job.login.clear().await;
                    let expires_at = manager.ready_expires_at(now());
                    let _ = task_job
                        .update(|status| {
                            status.phase = JobPhase::Paused;
                            status.message = reason.public_message(&manager.config);
                            status.logged_in = false;
                            status.masked_uin = None;
                            status.run_started_at = None;
                            status.last_progress_at = None;
                            status.queued_ahead = 0;
                            status.expires_at = expires_at;
                        })
                        .await;
                }
                (None, Ok(())) => {}
                (None, Err(_error)) if cancellation.is_cancelled() => {
                    let expires_at = manager.ready_expires_at(now());
                    let _ = task_job
                        .update(|status| {
                            status.phase = JobPhase::Cancelled;
                            status.message = "任务已取消；已完成的分页仍保留到任务到期".into();
                            status.run_started_at = None;
                            status.last_progress_at = None;
                            status.queued_ahead = 0;
                            status.expires_at = expires_at;
                        })
                        .await;
                }
                (None, Err(error)) => {
                    tracing::warn!(job = %short_id(&task_job.id), reason = %safe_log_error(&error), "archive failed");
                    let message = public_archive_error(&error);
                    task_job.login.clear().await;
                    let expires_at = manager.ready_expires_at(now());
                    let _ = task_job
                        .update(|status| {
                            status.phase = JobPhase::Failed;
                            status.message = message;
                            status.logged_in = false;
                            status.masked_uin = None;
                            status.run_started_at = None;
                            status.last_progress_at = None;
                            status.queued_ahead = 0;
                            status.expires_at = expires_at;
                        })
                        .await;
                }
            }
            drop(active_guard);
            drop(permit);
            manager.refresh_queue_positions().await;
        });
        job.replace_task(task).await;
        Ok(queued)
    }

    pub async fn delete(&self, id: &str) -> Result<(), AppError> {
        let job = self.jobs.read().await.get(id).cloned();
        let Some(job) = job else {
            return Ok(());
        };
        let _lifecycle_guard = job.lifecycle_lock.lock().await;
        let removed = {
            let mut jobs = self.jobs.write().await;
            if jobs
                .get(id)
                .is_some_and(|current| Arc::ptr_eq(current, &job))
            {
                jobs.remove(id)
            } else {
                None
            }
        };
        if removed.is_none() {
            return Ok(());
        }
        self.remove_from_queue(id).await;
        job.abort_task().await;
        job.login.clear().await;
        if job.dir.starts_with(&self.config.data_dir) {
            tokio::fs::remove_dir_all(&job.dir).await?;
        }
        self.refresh_queue_positions().await;
        Ok(())
    }

    pub async fn mark_downloaded(&self, job: &Arc<JobRuntime>) -> Result<JobStatus, AppError> {
        let expires_at = now() + self.config.post_download_ttl.as_secs() as i64;
        job.update(|status| {
            status.downloaded_at = Some(now());
            status.expires_at = status.expires_at.min(expires_at);
            status.message = format!(
                "下载已开始；服务器临时文件将在约 {} 分钟内自动删除",
                self.config.post_download_ttl.as_secs().div_ceil(60)
            );
        })
        .await
    }

    pub fn active_count(&self) -> usize {
        self.active_jobs.load(Ordering::Relaxed)
    }

    pub async fn stored_count(&self) -> usize {
        self.jobs.read().await.len()
    }

    pub fn storage_available(&self) -> u64 {
        fs2::available_space(&self.config.data_dir).unwrap_or(0)
    }

    pub fn idle_expires_at(&self, current: i64) -> i64 {
        current + self.config.idle_job_ttl.as_secs() as i64
    }

    pub fn ready_expires_at(&self, current: i64) -> i64 {
        current + self.config.ready_job_ttl.as_secs() as i64
    }

    async fn remove_from_queue(&self, id: &str) {
        let mut queue = self.queue.lock().await;
        let previous = queue.len();
        queue.retain(|queued| queued != id);
        if queue.len() != previous {
            self.queue_notify.notify_waiters();
        }
    }

    async fn acquire_archive_slot(
        &self,
        id: &str,
        cancellation: &CancellationToken,
    ) -> Option<OwnedSemaphorePermit> {
        loop {
            // Register for a notification before inspecting the queue to avoid a lost wake-up.
            let notified = self.queue_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let is_front = self
                .queue
                .lock()
                .await
                .front()
                .is_some_and(|queued| queued == id);
            if is_front {
                let permit = tokio::select! {
                    permit = self.archive_slots.clone().acquire_owned() => permit.ok(),
                    _ = cancellation.cancelled() => None,
                };
                self.remove_from_queue(id).await;
                return permit;
            }
            tokio::select! {
                _ = &mut notified => {}
                _ = cancellation.cancelled() => {
                    self.remove_from_queue(id).await;
                    return None;
                }
            }
        }
    }

    async fn refresh_queue_positions(&self) {
        let queued = self.queue.lock().await.iter().cloned().collect::<Vec<_>>();
        let active = self.active_count() as u64;
        for (index, id) in queued.iter().enumerate() {
            let job = self.jobs.read().await.get(id).cloned();
            let Some(job) = job else { continue };
            let ahead = active + index as u64;
            if job.status().await.queued_ahead != ahead {
                let _ = job.update(|status| status.queued_ahead = ahead).await;
            }
        }
    }

    pub async fn cleanup_expired(&self) {
        let current = now();
        let expired = {
            let jobs = self.jobs.read().await;
            let mut expired = Vec::new();
            for (id, job) in jobs.iter() {
                if job.status().await.expires_at <= current {
                    expired.push(id.clone());
                }
            }
            expired
        };
        for id in expired {
            if let Err(error) = self.delete(&id).await {
                tracing::warn!(job = %short_id(&id), code = error.code, "expired job cleanup failed");
            }
        }
    }

    pub fn spawn_cleanup(self: &Arc<Self>) {
        let manager = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                manager.cleanup_expired().await;
            }
        });
    }

    fn ensure_storage_available(&self) -> Result<(), AppError> {
        if self.storage_available() < self.config.min_free_bytes {
            return Err(AppError::unavailable(
                "insufficient_storage",
                "服务器剩余空间不足，暂时不能创建或继续任务",
            ));
        }
        Ok(())
    }

    async fn load_existing(&self) -> Result<(), AppError> {
        let mut entries = tokio::fs::read_dir(&self.config.data_dir).await?;
        let mut loaded = HashMap::new();
        while let Some(entry) = entries.next_entry().await? {
            let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let path = entry.path();
            if !entry.file_type().await?.is_dir() || !valid_job_id(&id) {
                continue;
            }
            let raw = match tokio::fs::read(path.join("status.json")).await {
                Ok(raw) => raw,
                Err(_) => {
                    quarantine_or_remove(&self.config.data_dir, &path).await;
                    continue;
                }
            };
            let mut persistent = match serde_json::from_slice::<PersistentJob>(&raw) {
                Ok(value)
                    if value.schema_version == 1
                        && value.status.job_id == id
                        && value.token_hash.len() == 64 =>
                {
                    value
                }
                _ => {
                    quarantine_or_remove(&self.config.data_dir, &path).await;
                    continue;
                }
            };
            if persistent.status.expires_at <= now() {
                let _ = tokio::fs::remove_dir_all(&path).await;
                continue;
            }
            persistent.status.logged_in = false;
            persistent.status.masked_uin = None;
            if persistent.status.last_activity_at == 0 {
                persistent.status.last_activity_at = persistent.status.created_at;
            }
            persistent.status.queued_ahead = 0;
            persistent.status.run_started_at = None;
            persistent.status.last_progress_at = None;
            if persistent.status.phase.is_active() {
                persistent.status.phase = JobPhase::Interrupted;
                persistent.status.message =
                    "服务器曾重启；已保存的数据仍在，请重新扫码后继续".into();
                persistent.status.expires_at = self.ready_expires_at(now());
            } else if matches!(
                persistent.status.phase,
                JobPhase::AwaitingLogin | JobPhase::LoggedIn
            ) {
                persistent.status.expires_at = persistent
                    .status
                    .expires_at
                    .min(self.idle_expires_at(now()));
            }
            let job = Arc::new(JobRuntime::new(
                id.clone(),
                path,
                persistent.token_hash,
                persistent.status,
            )?);
            job.persist().await?;
            loaded.insert(id, job);
        }
        *self.jobs.write().await = loaded;
        Ok(())
    }
}

struct ActiveJobGuard<'a>(&'a AtomicUsize);

impl Drop for ActiveJobGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WatchdogReason {
    NoProgress,
    RunLimit,
}

impl WatchdogReason {
    fn public_message(self, config: &Config) -> String {
        match self {
            Self::NoProgress => format!(
                "连续 {} 分钟没有取得新进度，已自动暂停并释放位置；已保存的分页还在",
                config.no_progress_timeout.as_secs().div_ceil(60)
            ),
            Self::RunLimit => format!(
                "本次运行已达到 {} 分钟，已自动暂停并释放位置；已保存的分页还在",
                config.max_run_duration.as_secs().div_ceil(60)
            ),
        }
    }
}

fn watchdog_reason(status: &JobStatus, current: i64, config: &Config) -> Option<WatchdogReason> {
    let started_at = status.run_started_at?;
    if current.saturating_sub(started_at) >= config.max_run_duration.as_secs() as i64 {
        return Some(WatchdogReason::RunLimit);
    }
    // Packaging can be CPU/disk-bound for several minutes without changing record counters.
    if status.phase != JobPhase::Packaging
        && status.last_progress_at.is_some_and(|last| {
            current.saturating_sub(last) >= config.no_progress_timeout.as_secs() as i64
        })
    {
        return Some(WatchdogReason::NoProgress);
    }
    None
}

fn public_archive_error(error: &str) -> String {
    if error.contains("登录") || error.contains("p_skey") {
        "QQ 登录已失效；已完成的数据仍保留，请重新扫码后继续".into()
    } else if error.contains("HTTP 5") || error.contains("重试") || error.contains("QQ 空间") {
        format!(
            "归档暂时中断：{}。已保存当前进度，可以稍后重试",
            concise(error)
        )
    } else if error.contains("空间不足") || error.contains("大小上限") {
        concise(error)
    } else {
        "归档暂时中断；已保存当前进度，请稍后重试".into()
    }
}

fn safe_log_error(error: &str) -> String {
    if error.contains("http") || error.contains("HTTP") {
        "upstream_http_error".into()
    } else if error.contains("登录") {
        "login_expired".into()
    } else if error.contains("数据库") {
        "job_database_error".into()
    } else {
        "archive_error".into()
    }
}

fn concise(value: &str) -> String {
    value.chars().take(160).collect()
}

fn short_id(id: &str) -> &str {
    id.get(..8).unwrap_or("invalid")
}

async fn quarantine_or_remove(root: &Path, path: &Path) {
    if path.starts_with(root) {
        let _ = tokio::fs::remove_dir_all(path).await;
    }
}

#[cfg(unix)]
async fn set_private_directory_permissions(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;

    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).await?;
    Ok(())
}

#[cfg(not(unix))]
async fn set_private_directory_permissions(_path: &Path) -> Result<(), AppError> {
    Ok(())
}

#[cfg(unix)]
async fn set_private_file_permissions(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;

    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await?;
    Ok(())
}

#[cfg(not(unix))]
async fn set_private_file_permissions(_path: &Path) -> Result<(), AppError> {
    Ok(())
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use std::{sync::atomic::Ordering, time::Duration};

    use tempfile::tempdir;

    use super::{now, watchdog_reason, JobManager, WatchdogReason};
    use crate::{config::Config, model::JobPhase};

    #[tokio::test]
    async fn owner_secret_is_required_for_job_access() {
        let directory = tempdir().unwrap();
        let config = Config::development(
            directory.path().join("jobs"),
            directory.path().join("public"),
        );
        let manager = JobManager::new(config).await.unwrap();
        let (job, owner) = manager.create().await.unwrap();
        assert!(manager.authorize(&job.id, &owner).await.is_ok());
        assert!(manager.authorize(&job.id, &"0".repeat(64)).await.is_err());
    }

    #[tokio::test]
    async fn concurrent_creates_respect_capacity() {
        let directory = tempdir().unwrap();
        let mut config = Config::development(
            directory.path().join("jobs"),
            directory.path().join("public"),
        );
        config.max_jobs = 2;
        let manager = JobManager::new(config).await.unwrap();
        let mut attempts = Vec::new();
        for _ in 0..12 {
            let manager = manager.clone();
            attempts.push(tokio::spawn(async move { manager.create().await.is_ok() }));
        }
        let mut created = 0;
        for attempt in attempts {
            if attempt.await.unwrap() {
                created += 1;
            }
        }
        assert_eq!(created, 2);
        assert_eq!(manager.stored_count().await, 2);
    }

    #[tokio::test]
    async fn queue_positions_include_the_active_archive() {
        let directory = tempdir().unwrap();
        let config = Config::development(
            directory.path().join("jobs"),
            directory.path().join("public"),
        );
        let manager = JobManager::new(config).await.unwrap();
        let (first, _) = manager.create().await.unwrap();
        let (second, _) = manager.create().await.unwrap();
        manager.active_jobs.store(1, Ordering::Relaxed);
        manager.queue.lock().await.push_back(first.id.clone());
        manager.queue.lock().await.push_back(second.id.clone());

        manager.refresh_queue_positions().await;

        assert_eq!(first.status().await.queued_ahead, 1);
        assert_eq!(second.status().await.queued_ahead, 2);
    }

    #[tokio::test]
    async fn fifo_queue_cannot_be_overtaken_by_a_later_waiter() {
        let directory = tempdir().unwrap();
        let config = Config::development(
            directory.path().join("jobs"),
            directory.path().join("public"),
        );
        let manager = JobManager::new(config).await.unwrap();
        let blocking_permit = manager.archive_slots.clone().acquire_owned().await.unwrap();
        manager.queue.lock().await.push_back("first".into());
        manager.queue.lock().await.push_back("second".into());

        let second_manager = manager.clone();
        let second_cancel = tokio_util::sync::CancellationToken::new();
        let second_waiter = tokio::spawn(async move {
            second_manager
                .acquire_archive_slot("second", &second_cancel)
                .await
        });
        let first_manager = manager.clone();
        let first_cancel = tokio_util::sync::CancellationToken::new();
        let first_waiter = tokio::spawn(async move {
            first_manager
                .acquire_archive_slot("first", &first_cancel)
                .await
        });
        drop(blocking_permit);

        let first_permit = tokio::time::timeout(Duration::from_secs(1), first_waiter)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!second_waiter.is_finished());
        drop(first_permit);
        let second_permit = tokio::time::timeout(Duration::from_secs(1), second_waiter)
            .await
            .unwrap()
            .unwrap();
        assert!(second_permit.is_some());
    }

    #[tokio::test]
    async fn expired_idle_jobs_release_stored_capacity() {
        let directory = tempdir().unwrap();
        let config = Config::development(
            directory.path().join("jobs"),
            directory.path().join("public"),
        );
        let manager = JobManager::new(config).await.unwrap();
        let (job, _) = manager.create().await.unwrap();
        job.update(|status| status.expires_at = now() - 1)
            .await
            .unwrap();

        manager.cleanup_expired().await;

        assert_eq!(manager.stored_count().await, 0);
        assert!(!job.dir.exists());
    }

    #[test]
    fn watchdog_pauses_stalled_work_but_not_packaging() {
        let directory = tempdir().unwrap();
        let mut config = Config::development(
            directory.path().join("jobs"),
            directory.path().join("public"),
        );
        config.no_progress_timeout = Duration::from_secs(300);
        config.max_run_duration = Duration::from_secs(3_600);
        let current = 10_000;
        let mut status = crate::model::JobStatus::new("job".into(), 1, 20_000);
        status.phase = JobPhase::Archiving;
        status.run_started_at = Some(current - 1_000);
        status.last_progress_at = Some(current - 301);

        assert_eq!(
            watchdog_reason(&status, current, &config),
            Some(WatchdogReason::NoProgress)
        );
        status.phase = JobPhase::Packaging;
        assert_eq!(watchdog_reason(&status, current, &config), None);
        status.run_started_at = Some(current - 3_601);
        assert_eq!(
            watchdog_reason(&status, current, &config),
            Some(WatchdogReason::RunLimit)
        );
    }
}
