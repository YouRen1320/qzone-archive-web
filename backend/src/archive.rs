use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use futures_util::StreamExt;
use reqwest::{
    header::{ACCEPT, ACCEPT_LANGUAGE, CONTENT_TYPE, COOKIE, REFERER, USER_AGENT},
    redirect::Policy,
};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;
use url::Url;
use walkdir::WalkDir;

use crate::{
    config::Config,
    database, export,
    job::JobRuntime,
    model::{JobPhase, StartArchiveRequest},
    qq::feeds,
};

const CURSOR_MAX_AGE_SECONDS: i64 = 10 * 60;
const MAX_IMAGE_BYTES: u64 = 50 * 1024 * 1024;
const MAX_VIDEO_BYTES: u64 = 500 * 1024 * 1024;

pub async fn run(
    job: Arc<JobRuntime>,
    config: Config,
    request: StartArchiveRequest,
    cancellation: CancellationToken,
) -> Result<(), String> {
    let auth = job.login.auth().await?;
    let owner_uin = auth.uin.clone();
    let db_path = job.db_path();
    let initialize_path = db_path.clone();
    tokio::task::spawn_blocking(move || database::initialize(&initialize_path))
        .await
        .map_err(|_| "初始化任务数据库意外停止".to_owned())??;

    let checkpoint_path = db_path.clone();
    let checkpoint_owner = owner_uin.clone();
    let mut checkpoint = tokio::task::spawn_blocking(move || {
        database::load_checkpoint(&checkpoint_path, &checkpoint_owner)
    })
    .await
    .map_err(|_| "读取归档断点意外停止".to_owned())??;
    if checkpoint
        .as_ref()
        .is_some_and(|value| now().saturating_sub(value.updated_at) > CURSOR_MAX_AGE_SECONDS)
    {
        let clear_path = db_path.clone();
        let clear_owner = owner_uin.clone();
        tokio::task::spawn_blocking(move || database::clear_checkpoint(&clear_path, &clear_owner))
            .await
            .map_err(|_| "清理过期归档断点意外停止".to_owned())??;
        checkpoint = None;
    }

    let mut cursor = checkpoint.as_ref().map(|value| value.cursor.clone());
    let mut seen_cursors = HashSet::new();
    if let Some(cursor) = cursor.as_ref() {
        seen_cursors.insert(cursor.clone());
    }
    job.update(|status| {
        status.phase = JobPhase::Archiving;
        status.last_progress_at = Some(now());
        if let Some(checkpoint) = checkpoint.as_ref() {
            status.pages = status.pages.max(checkpoint.pages);
            status.fetched = status.fetched.max(checkpoint.fetched);
            status.saved = status.saved.max(checkpoint.saved);
            status.message = format!(
                "已恢复断点：{} 页、{} 条记录，正在继续",
                checkpoint.pages, checkpoint.saved
            );
        } else {
            status.message = "已进入安全执行槽，正在读取 QQ 空间互动记录".into();
        }
    })
    .await
    .map_err(|error| error.message)?;

    let mut pages_this_run = 0_u64;
    let mut complete = false;
    loop {
        ensure_not_cancelled(&cancellation)?;
        ensure_capacity(&job.dir, &config)?;
        let page = tokio::select! {
            result = feeds::fetch_feeds(
                &job.login,
                if cursor.is_some() { "2" } else { "1" },
                cursor.as_deref(),
            ) => result?,
            _ = cancellation.cancelled() => return Err("任务已取消".into()),
        };
        let next = if page.has_more {
            Some(
                page.attach_info
                    .as_deref()
                    .ok_or("QQ 表示仍有数据，但没有返回下一页游标")?
                    .to_owned(),
            )
        } else {
            None
        };
        if let Some(next_cursor) = next.as_ref() {
            if !seen_cursors.insert(next_cursor.clone()) {
                return Err("QQ 返回了重复分页游标，已安全停止以避免死循环".into());
            }
        }
        let save_path = db_path.clone();
        let save_owner = owner_uin.clone();
        let save_feeds = page.feeds;
        let save_next = next.clone();
        let saved = tokio::task::spawn_blocking(move || {
            database::save_page(&save_path, &save_owner, &save_feeds, save_next.as_deref())
        })
        .await
        .map_err(|_| "保存归档分页意外停止".to_owned())??;
        pages_this_run += 1;
        job.update(|status| {
            status.pages += 1;
            status.fetched += saved.processed;
            status.saved = saved.unique_feeds;
            status.media_total = saved.media_total;
            status.last_progress_at = Some(now());
            status.message = format!(
                "已归档 {} 页，共整理 {} 条唯一记录",
                status.pages, status.saved
            );
        })
        .await
        .map_err(|error| error.message)?;

        if next.is_none() {
            complete = true;
            break;
        }
        cursor = next;
        if request
            .max_pages
            .is_some_and(|maximum| pages_this_run >= maximum)
        {
            break;
        }
        let delay = jittered_delay(request.page_delay_ms);
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_millis(delay)) => {},
            _ = cancellation.cancelled() => return Err("任务已取消".into()),
        }
    }

    if request.include_media && job.status().await.media_total > 0 {
        job.update(|status| {
            status.phase = JobPhase::DownloadingMedia;
            status.message = format!("正在下载 {} 个媒体文件", status.media_total);
            status.last_progress_at = Some(now());
        })
        .await
        .map_err(|error| error.message)?;
        download_media(&job, &config, &owner_uin, &cancellation).await?;
    }

    ensure_not_cancelled(&cancellation)?;
    ensure_capacity(&job.dir, &config)?;
    job.update(|status| {
        status.phase = JobPhase::Packaging;
        status.message = "正在生成可离线浏览的 ZIP 文件".into();
        status.last_progress_at = Some(now());
    })
    .await
    .map_err(|error| error.message)?;
    let path = export::package(job.clone(), owner_uin, complete, cancellation.clone()).await?;
    let size = tokio::fs::metadata(&path)
        .await
        .map_err(|error| format!("检查导出文件失败：{error}"))?
        .len();
    if size == 0 {
        return Err("导出 ZIP 为空，已拒绝提供下载".into());
    }
    job.login.clear().await;
    let ready_expires_at = now() + config.ready_job_ttl.as_secs() as i64;
    job.update(|status| {
        status.phase = JobPhase::Ready;
        status.download_ready = true;
        status.logged_in = false;
        status.masked_uin = None;
        status.run_started_at = None;
        status.last_progress_at = None;
        status.queued_ahead = 0;
        status.expires_at = ready_expires_at;
        status.message = if complete {
            format!(
                "归档完成：{} 条记录，{} 个媒体文件可下载",
                status.saved, status.media_downloaded
            )
        } else {
            format!(
                "已按本次页数限制生成部分归档：{} 条记录，可下载或重新扫码继续",
                status.saved
            )
        };
    })
    .await
    .map_err(|error| error.message)?;
    Ok(())
}

async fn download_media(
    job: &Arc<JobRuntime>,
    config: &Config,
    owner_uin: &str,
    cancellation: &CancellationToken,
) -> Result<(), String> {
    tokio::fs::create_dir_all(job.media_dir())
        .await
        .map_err(|error| format!("创建媒体目录失败：{error}"))?;
    let query_path = job.db_path();
    let query_owner = owner_uin.to_owned();
    let entries =
        tokio::task::spawn_blocking(move || database::pending_media(&query_path, &query_owner))
            .await
            .map_err(|_| "读取媒体下载清单意外停止".to_owned())??;
    let auth = job.login.auth().await?;
    let client = reqwest::Client::builder()
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                attempt.error("too many redirects")
            } else if allowed_media_url(attempt.url()) {
                attempt.follow()
            } else {
                attempt.error("redirect target is not an approved QQ media host")
            }
        }))
        .connect_timeout(std::time::Duration::from_secs(20))
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|error| format!("创建媒体下载客户端失败：{error}"))?;
    let downloader = MediaDownloader {
        client: &client,
        auth: &auth,
        media_dir: job.media_dir(),
        cancellation,
    };

    for entry in entries {
        ensure_not_cancelled(cancellation)?;
        if ensure_capacity(&job.dir, config).is_err() {
            job.update(|status| {
                status.message = "已接近任务或服务器空间上限，停止下载剩余媒体并继续打包".into();
            })
            .await
            .map_err(|error| error.message)?;
            break;
        }
        let maximum = if entry.kind == "video" {
            MAX_VIDEO_BYTES
        } else {
            MAX_IMAGE_BYTES
        };
        match downloader
            .download_one(&entry.candidates, &entry.kind, entry.id, maximum)
            .await
        {
            Ok((path, bytes)) => {
                let relative = format!(
                    "media/{}",
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .ok_or("媒体文件名无效")?
                );
                let database_path = job.db_path();
                tokio::task::spawn_blocking(move || {
                    database::mark_media_downloaded(&database_path, entry.id, &relative, bytes)
                })
                .await
                .map_err(|_| "更新媒体状态意外停止".to_owned())??;
                job.update(|status| {
                    status.media_downloaded += 1;
                    status.last_progress_at = Some(now());
                    status.message = format!(
                        "媒体下载进度：{}/{}",
                        status.media_downloaded + status.media_failed,
                        status.media_total
                    );
                })
                .await
                .map_err(|error| error.message)?;
            }
            Err(error) if cancellation.is_cancelled() => return Err(error),
            Err(error) => {
                let database_path = job.db_path();
                let safe = media_failure_reason(&error);
                tokio::task::spawn_blocking(move || {
                    database::mark_media_failed(&database_path, entry.id, &safe)
                })
                .await
                .map_err(|_| "更新媒体失败状态意外停止".to_owned())??;
                job.update(|status| {
                    status.media_failed += 1;
                    status.last_progress_at = Some(now());
                    status.message = format!(
                        "媒体下载进度：{}/{}（{} 个失败）",
                        status.media_downloaded + status.media_failed,
                        status.media_total,
                        status.media_failed
                    );
                })
                .await
                .map_err(|error| error.message)?;
            }
        }
    }
    Ok(())
}

struct MediaDownloader<'a> {
    client: &'a reqwest::Client,
    auth: &'a crate::qq::login::QzoneAuth,
    media_dir: PathBuf,
    cancellation: &'a CancellationToken,
}

impl MediaDownloader<'_> {
    async fn download_one(
        &self,
        candidates: &[String],
        kind: &str,
        id: i64,
        maximum: u64,
    ) -> Result<(PathBuf, u64), String> {
        for raw_url in candidates {
            let Some(url) = normalized_media_url(raw_url) else {
                continue;
            };
            let mut request = self
                .client
                .get(url.clone())
                .header(USER_AGENT, &self.auth.user_agent)
                .header(ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9,en;q=0.8")
                .header(REFERER, "https://user.qzone.qq.com/")
                .header(
                    ACCEPT,
                    if kind == "video" {
                        "video/*,application/octet-stream;q=0.8,*/*;q=0.5"
                    } else {
                        "image/avif,image/webp,image/png,image/jpeg,image/*,*/*;q=0.5"
                    },
                );
            if cookie_allowed_for_host(&url) {
                request = request.header(COOKIE, &self.auth.cookie_header);
            }
            let response = match tokio::select! {
                response = request.send() => response,
                _ = self.cancellation.cancelled() => return Err("任务已取消".into()),
            } {
                Ok(response) if response.status().is_success() => response,
                _ => continue,
            };
            if response
                .content_length()
                .is_some_and(|length| length > maximum)
            {
                continue;
            }
            let content_type = response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_owned();
            let temporary = self.media_dir.join(format!(".{id}.part"));
            let _ = tokio::fs::remove_file(&temporary).await;
            let mut file = tokio::fs::File::create(&temporary)
                .await
                .map_err(|error| format!("创建媒体暂存文件失败：{error}"))?;
            let mut stream = response.bytes_stream();
            let mut written = 0_u64;
            let mut failed = false;
            loop {
                let next = tokio::select! {
                    chunk = stream.next() => chunk,
                    _ = self.cancellation.cancelled() => {
                        let _ = tokio::fs::remove_file(&temporary).await;
                        return Err("任务已取消".into());
                    }
                };
                let Some(chunk) = next else {
                    break;
                };
                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(_) => {
                        failed = true;
                        break;
                    }
                };
                written = written.saturating_add(chunk.len() as u64);
                if written > maximum {
                    failed = true;
                    break;
                }
                if file.write_all(&chunk).await.is_err() {
                    failed = true;
                    break;
                }
            }
            drop(file);
            if failed || written < 16 {
                let _ = tokio::fs::remove_file(&temporary).await;
                continue;
            }
            let header = read_header(&temporary).await?;
            if kind == "image" && is_missing_image_placeholder(&temporary, &header).await {
                let _ = tokio::fs::remove_file(&temporary).await;
                continue;
            }
            let Some(extension) = media_extension(kind, &content_type, &url, &header) else {
                let _ = tokio::fs::remove_file(&temporary).await;
                continue;
            };
            let final_path = self.media_dir.join(format!("{id}.{extension}"));
            tokio::fs::rename(&temporary, &final_path)
                .await
                .map_err(|error| format!("保存媒体文件失败：{error}"))?;
            return Ok((final_path, written));
        }
        Err("所有可用的 QQ 媒体地址都下载失败或已经失效".into())
    }
}

fn allowed_media_url(url: &Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    allowed_media_host(url)
}

fn allowed_media_host(url: &Url) -> bool {
    let Some(host) = url.host_str().map(str::to_ascii_lowercase) else {
        return false;
    };
    ["qq.com", "qpic.cn", "gtimg.cn", "qlogo.cn"]
        .iter()
        .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
}

fn normalized_media_url(raw: &str) -> Option<Url> {
    let mut url = Url::parse(raw).ok()?;
    if url.scheme() == "http" {
        if !allowed_media_host(&url) {
            return None;
        }
        url.set_scheme("https").ok()?;
    }
    allowed_media_url(&url).then_some(url)
}

fn cookie_allowed_for_host(url: &Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    url.host_str()
        .map(str::to_ascii_lowercase)
        .is_some_and(|host| host == "qq.com" || host.ends_with(".qq.com"))
}

fn media_extension(
    kind: &str,
    content_type: &str,
    url: &Url,
    header: &[u8],
) -> Option<&'static str> {
    if kind == "image" {
        if header.starts_with(&[0xff, 0xd8, 0xff]) {
            return Some("jpg");
        }
        if header.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Some("png");
        }
        if header.starts_with(b"GIF87a") || header.starts_with(b"GIF89a") {
            return Some("gif");
        }
        if header.len() >= 12 && header.starts_with(b"RIFF") && &header[8..12] == b"WEBP" {
            return Some("webp");
        }
        return None;
    }
    let content_type = content_type.to_ascii_lowercase();
    if content_type.contains("webm") {
        Some("webm")
    } else if content_type.contains("quicktime") {
        Some("mov")
    } else if content_type.contains("mp4") || header.get(4..8) == Some(b"ftyp") {
        Some("mp4")
    } else {
        url.path().rsplit('.').next().and_then(|extension| {
            match extension.to_ascii_lowercase().as_str() {
                "mp4" | "m4v" => Some("mp4"),
                "mov" => Some("mov"),
                "webm" => Some("webm"),
                _ => None,
            }
        })
    }
}

async fn read_header(path: &Path) -> Result<Vec<u8>, String> {
    use tokio::io::AsyncReadExt;

    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| format!("检查媒体格式失败：{error}"))?;
    let mut header = vec![0_u8; 32];
    let read = file
        .read(&mut header)
        .await
        .map_err(|error| format!("检查媒体格式失败：{error}"))?;
    header.truncate(read);
    Ok(header)
}

async fn is_missing_image_placeholder(path: &Path, header: &[u8]) -> bool {
    if !(header.starts_with(b"GIF87a") || header.starts_with(b"GIF89a")) {
        return false;
    }
    let Ok(metadata) = tokio::fs::metadata(path).await else {
        return false;
    };
    if ![1_547_u64, 1_643, 2_038, 2_687].contains(&metadata.len()) || header.len() < 10 {
        return false;
    }
    let width = u16::from_le_bytes([header[6], header[7]]);
    let height = u16::from_le_bytes([header[8], header[9]]);
    matches!((width, height), (340, 320) | (99, 99) | (98, 98))
}

fn ensure_capacity(job_dir: &Path, config: &Config) -> Result<(), String> {
    let used = directory_size(job_dir)?;
    if used >= config.max_job_bytes {
        return Err("任务数据已达到大小上限，媒体下载已停止".into());
    }
    if fs2::available_space(&config.data_dir).unwrap_or(0) < config.min_free_bytes {
        return Err("服务器剩余空间不足，任务已安全暂停".into());
    }
    Ok(())
}

fn directory_size(path: &Path) -> Result<u64, String> {
    let mut total = 0_u64;
    for entry in WalkDir::new(path).follow_links(false) {
        let entry = entry.map_err(|error| format!("检查任务大小失败：{error}"))?;
        if entry.file_type().is_file() {
            total = total.saturating_add(
                entry
                    .metadata()
                    .map_err(|error| format!("检查任务文件大小失败：{error}"))?
                    .len(),
            );
        }
    }
    Ok(total)
}

fn ensure_not_cancelled(cancellation: &CancellationToken) -> Result<(), String> {
    if cancellation.is_cancelled() {
        Err("任务已取消".into())
    } else {
        Ok(())
    }
}

fn jittered_delay(base: u64) -> u64 {
    let base = base.clamp(2_000, 30_000);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    base + nanos % ((base / 4).max(1) + 1)
}

fn media_failure_reason(error: &str) -> String {
    if error.contains("大小") {
        "媒体文件超过安全大小限制".into()
    } else {
        "QQ 媒体地址已经失效或暂时不可访问".into()
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::{
        allowed_media_url, cookie_allowed_for_host, media_extension, normalized_media_url,
    };

    #[test]
    fn blocks_non_qq_hosts_and_local_ssrf_targets() {
        assert!(allowed_media_url(
            &Url::parse("https://m.qpic.cn/example.jpg").unwrap()
        ));
        assert!(!allowed_media_url(
            &Url::parse("http://video.qq.com/example.mp4").unwrap()
        ));
        assert!(!allowed_media_url(
            &Url::parse("http://127.0.0.1/admin").unwrap()
        ));
        assert!(!allowed_media_url(
            &Url::parse("https://qq.com.evil.invalid/file").unwrap()
        ));
        assert_eq!(
            normalized_media_url("http://video.qq.com/example.mp4")
                .unwrap()
                .as_str(),
            "https://video.qq.com/example.mp4"
        );
        assert!(normalized_media_url("http://127.0.0.1/admin").is_none());
        assert!(cookie_allowed_for_host(
            &Url::parse("https://video.qq.com/example.mp4").unwrap()
        ));
        assert!(!cookie_allowed_for_host(
            &Url::parse("http://video.qq.com/example.mp4").unwrap()
        ));
    }

    #[test]
    fn detects_supported_media_from_magic_bytes() {
        let url = Url::parse("https://m.qpic.cn/file").unwrap();
        assert_eq!(
            media_extension("image", "", &url, b"\x89PNG\r\n\x1a\nrest"),
            Some("png")
        );
        assert_eq!(
            media_extension("video", "video/mp4", &url, b"0000ftyp"),
            Some("mp4")
        );
    }
}
