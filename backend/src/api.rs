use std::{convert::Infallible, io::SeekFrom, path::PathBuf, sync::Arc};

use axum::{
    body::Body,
    extract::{Path as AxumPath, Query, State},
    http::{
        header::{
            ACCEPT_RANGES, CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE,
            CONTENT_TYPE, ORIGIN, RANGE, X_CONTENT_TYPE_OPTIONS,
        },
        HeaderMap, HeaderValue, Method, Request, StatusCode,
    },
    middleware::{self, Next},
    response::{sse::Event, IntoResponse, Response, Sse},
    routing::{get, post},
    Json, Router,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use futures_util::{stream, Stream, StreamExt};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream};
use tokio_util::io::ReaderStream;
use tower_http::{
    compression::CompressionLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

use crate::{
    database::{self, ViewerRecordsQuery},
    error::AppError,
    job::{JobManager, JobRuntime},
    model::{HealthResponse, JobPhase, JobStatus, LoginResponse, QrResponse, StartArchiveRequest},
};

const JOB_COOKIE: &str = "qzone_job";
const OWNER_COOKIE: &str = "qzone_owner";

#[derive(Clone)]
pub struct AppState {
    pub manager: Arc<JobManager>,
}

pub fn router(manager: Arc<JobManager>) -> Router {
    let state = AppState { manager };
    let api = Router::new()
        .route("/health", get(health))
        .route("/jobs", post(create_job))
        .route("/job", get(get_job).delete(delete_job))
        .route("/login/qr", post(start_qr_login))
        .route("/login/poll", post(poll_qr_login))
        .route("/archive", post(start_archive))
        .route("/archive/cancel", post(cancel_archive))
        .route("/archive/viewer/manifest", get(viewer_manifest))
        .route("/archive/viewer/records", get(viewer_records))
        .route("/archive/viewer/media/{*path}", get(viewer_media))
        .route("/events", get(events))
        .route("/download", get(download));
    let frontend = state.manager.config.frontend_dir.clone();
    let serve = ServeDir::new(&frontend)
        .append_index_html_on_directories(true)
        .not_found_service(ServeFile::new(frontend.join("index.html")));
    Router::new()
        .nest("/api", api)
        .fallback_service(serve)
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security_middleware,
        ))
        .with_state(state)
}

async fn create_job(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<(StatusCode, CookieJar, Json<JobStatus>), AppError> {
    if let Ok(existing) = authorized_job(&state, &jar).await {
        return Ok((StatusCode::OK, jar, Json(existing.status().await)));
    }
    let (job, owner_token) = state.manager.create().await?;
    let max_age = time::Duration::seconds(state.manager.config.job_ttl.as_secs() as i64);
    let jar = jar
        .add(private_cookie(
            JOB_COOKIE,
            job.id.clone(),
            max_age,
            state.manager.config.secure_cookies,
        ))
        .add(private_cookie(
            OWNER_COOKIE,
            owner_token,
            max_age,
            state.manager.config.secure_cookies,
        ));
    Ok((StatusCode::CREATED, jar, Json(job.status().await)))
}

async fn get_job(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<JobStatus>, AppError> {
    let job = authorized_job(&state, &jar).await?;
    Ok(Json(job.status().await))
}

async fn delete_job(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<(StatusCode, CookieJar), AppError> {
    let job = authorized_job(&state, &jar).await?;
    state.manager.delete(&job.id).await?;
    Ok((StatusCode::NO_CONTENT, clear_private_cookies(jar)))
}

async fn start_qr_login(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<QrResponse>, AppError> {
    let job = authorized_job(&state, &jar).await?;
    // Login refresh and archive start share one lifecycle boundary per job.
    let _lifecycle_guard = job.lifecycle_lock.lock().await;
    if job.status().await.phase.is_active() {
        return Err(AppError::conflict(
            "job_running",
            "任务运行期间不能更换 QQ 登录",
        ));
    }
    let qr = job.login.start_qr_login().await.map_err(upstream_error)?;
    job.update(|status| {
        status.phase = JobPhase::AwaitingLogin;
        status.logged_in = false;
        status.masked_uin = None;
        status.message = "请使用手机 QQ 扫描二维码并确认".into();
    })
    .await?;
    Ok(Json(QrResponse {
        qr_image: qr.qr_image,
        message: "二维码只用于本次临时任务，登录凭证不会写入磁盘".into(),
    }))
}

async fn poll_qr_login(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<LoginResponse>, AppError> {
    let job = authorized_job(&state, &jar).await?;
    let _lifecycle_guard = job.lifecycle_lock.lock().await;
    if job.status().await.phase.is_active() {
        return Err(AppError::conflict(
            "job_running",
            "任务运行期间不能更新 QQ 登录状态",
        ));
    }
    let login = job.login.poll_qr_login().await.map_err(upstream_error)?;
    if login.status == "success" {
        let masked = login.masked_uin.clone();
        job.update(|status| {
            status.phase = JobPhase::LoggedIn;
            status.logged_in = true;
            status.masked_uin = masked;
            status.message = "QQ 登录成功，可以开始归档".into();
        })
        .await?;
    } else {
        let message = login.message.clone();
        job.update(|status| status.message = message).await?;
    }
    Ok(Json(LoginResponse {
        status: login.status.into(),
        message: login.message,
        masked_uin: login.masked_uin,
    }))
}

async fn start_archive(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(request): Json<StartArchiveRequest>,
) -> Result<(StatusCode, Json<JobStatus>), AppError> {
    let job = authorized_job(&state, &jar).await?;
    let status = state.manager.start_archive(job, request).await?;
    Ok((StatusCode::ACCEPTED, Json(status)))
}

async fn cancel_archive(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<(StatusCode, Json<JobStatus>), AppError> {
    let job = authorized_job(&state, &jar).await?;
    if !job.status().await.phase.is_active() {
        return Err(AppError::conflict(
            "job_not_running",
            "当前没有正在运行的归档任务",
        ));
    }
    job.cancel().await;
    let status = job
        .update(|status| status.message = "正在安全停止任务，请稍候".into())
        .await?;
    Ok((StatusCode::ACCEPTED, Json(status)))
}

async fn events(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, AppError> {
    let job = authorized_job(&state, &jar).await?;
    let receiver = job.subscribe();
    let initial_job = job.clone();
    let initial = stream::once(async move {
        Ok(Event::default()
            .event("status")
            .json_data(initial_job.status().await)
            .unwrap_or_else(|_| Event::default().event("error").data("serialization_failed")))
    });
    let updates = BroadcastStream::new(receiver).filter_map(|result| async move {
        match result {
            Ok(status) => Some(Ok(Event::default()
                .event("status")
                .json_data(status)
                .unwrap_or_else(|_| {
                    Event::default().event("error").data("serialization_failed")
                }))),
            Err(BroadcastStreamRecvError::Lagged(_)) => None,
        }
    });
    Ok(Sse::new(initial.chain(updates)).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("keep-alive"),
    ))
}

async fn download(State(state): State<AppState>, jar: CookieJar) -> Result<Response, AppError> {
    let job = authorized_job(&state, &jar).await?;
    let status = job.status().await;
    if !status.download_ready || status.phase != JobPhase::Ready {
        return Err(AppError::conflict(
            "download_not_ready",
            "归档文件尚未准备完成",
        ));
    }
    let path = job.download_path();
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|_| AppError::internal("归档文件不存在或已经被清理"))?;
    let size = file.metadata().await?.len();
    state.manager.mark_downloaded(&job).await?;
    let mut response = Response::new(Body::from_stream(ReaderStream::new(file)));
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/zip"));
    response.headers_mut().insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"qzone-archive.zip\""),
    );
    response.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, no-store, max-age=0"),
    );
    response.headers_mut().insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&size.to_string())
            .map_err(|_| AppError::internal("归档文件大小无效"))?,
    );
    Ok(response)
}

async fn viewer_manifest(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Response, AppError> {
    let job = ready_job(&state, &jar).await?;
    stream_private_file(
        job.viewer_manifest_path(),
        "application/json; charset=utf-8",
        None,
    )
    .await
}

async fn viewer_records(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<ViewerQuery>,
) -> Result<Json<database::ViewerRecordsPage>, AppError> {
    let job = ready_job(&state, &jar).await?;
    let search = query
        .q
        .map(|value| value.trim().chars().take(100).collect::<String>())
        .filter(|value| !value.is_empty());
    let category = query.category.filter(|value| !value.is_empty());
    if category
        .as_deref()
        .is_some_and(|value| !matches!(value, "self" | "other" | "guestbook"))
    {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "invalid_viewer_category",
            "记录分类无效",
        ));
    }
    if query
        .year
        .is_some_and(|year| !(1970..=3000).contains(&year))
    {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "invalid_viewer_year",
            "记录年份无效",
        ));
    }
    let request = ViewerRecordsQuery {
        offset: query.offset.unwrap_or(0),
        limit: query.limit.unwrap_or(30).clamp(1, 60),
        search,
        category,
        year: query.year,
    };
    let path = job.db_path();
    let page = tokio::task::spawn_blocking(move || database::viewer_records(&path, request))
        .await
        .map_err(|_| AppError::internal("阅读记录查询意外停止"))?
        .map_err(AppError::internal)?;
    Ok(Json(page))
}

#[derive(Debug, Deserialize)]
struct ViewerQuery {
    offset: Option<u64>,
    limit: Option<u64>,
    q: Option<String>,
    category: Option<String>,
    year: Option<i32>,
}

async fn viewer_media(
    State(state): State<AppState>,
    jar: CookieJar,
    AxumPath(path): AxumPath<String>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let job = ready_job(&state, &jar).await?;
    if path.is_empty() || path.contains('\\') {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "invalid_media_path",
            "媒体文件路径无效",
        ));
    }
    let media_root = tokio::fs::canonicalize(job.media_dir())
        .await
        .map_err(|_| AppError::internal("媒体目录已经被清理"))?;
    let requested = tokio::fs::canonicalize(job.media_dir().join(&path))
        .await
        .map_err(|_| AppError::new(StatusCode::NOT_FOUND, "media_not_found", "媒体文件不存在"))?;
    if !requested.starts_with(&media_root) || !tokio::fs::metadata(&requested).await?.is_file() {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            "media_not_found",
            "媒体文件不存在",
        ));
    }
    let content_type = media_content_type(&requested);
    stream_private_file(requested, content_type, headers.get(RANGE)).await
}

async fn ready_job(state: &AppState, jar: &CookieJar) -> Result<Arc<JobRuntime>, AppError> {
    let job = authorized_job(state, jar).await?;
    let status = job.status().await;
    if status.phase != JobPhase::Ready || !status.download_ready {
        return Err(AppError::conflict("viewer_not_ready", "回忆册尚未准备完成"));
    }
    Ok(job)
}

async fn stream_private_file(
    path: PathBuf,
    content_type: &'static str,
    range: Option<&HeaderValue>,
) -> Result<Response, AppError> {
    let mut file = tokio::fs::File::open(&path)
        .await
        .map_err(|_| AppError::new(StatusCode::NOT_FOUND, "file_not_found", "文件已经被清理"))?;
    let size = file.metadata().await?.len();
    let range = parse_byte_range(range.and_then(|value| value.to_str().ok()), size)?;
    let (status, start, end) = match range {
        Some((start, end)) => (StatusCode::PARTIAL_CONTENT, start, end),
        None => (StatusCode::OK, 0, size.saturating_sub(1)),
    };
    let length = if size == 0 { 0 } else { end - start + 1 };
    if start > 0 {
        file.seek(SeekFrom::Start(start)).await?;
    }
    let stream = ReaderStream::new(file.take(length));
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
        .headers_mut()
        .insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    response.headers_mut().insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&length.to_string())
            .map_err(|_| AppError::internal("文件大小无效"))?,
    );
    response.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, no-store, max-age=0"),
    );
    if status == StatusCode::PARTIAL_CONTENT {
        response.headers_mut().insert(
            CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes {start}-{end}/{size}"))
                .map_err(|_| AppError::internal("媒体范围无效"))?,
        );
    }
    Ok(response)
}

fn parse_byte_range(value: Option<&str>, size: u64) -> Result<Option<(u64, u64)>, AppError> {
    let Some(value) = value else { return Ok(None) };
    let Some(specification) = value.strip_prefix("bytes=") else {
        return Err(invalid_range());
    };
    if size == 0 || specification.contains(',') {
        return Err(invalid_range());
    }
    let Some((start, end)) = specification.split_once('-') else {
        return Err(invalid_range());
    };
    let range = if start.is_empty() {
        let suffix = end.parse::<u64>().map_err(|_| invalid_range())?.min(size);
        if suffix == 0 {
            return Err(invalid_range());
        }
        (size - suffix, size - 1)
    } else {
        let start = start.parse::<u64>().map_err(|_| invalid_range())?;
        if start >= size {
            return Err(invalid_range());
        }
        let end = if end.is_empty() {
            size - 1
        } else {
            end.parse::<u64>()
                .map_err(|_| invalid_range())?
                .min(size - 1)
        };
        if end < start {
            return Err(invalid_range());
        }
        (start, end)
    };
    Ok(Some(range))
}

fn invalid_range() -> AppError {
    AppError::new(
        StatusCode::RANGE_NOT_SATISFIABLE,
        "invalid_range",
        "请求的媒体范围无效",
    )
}

fn media_content_type(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        active_jobs: state.manager.active_count(),
        stored_jobs: state.manager.stored_count().await,
        storage_available_bytes: state.manager.storage_available(),
    })
}

async fn authorized_job(state: &AppState, jar: &CookieJar) -> Result<Arc<JobRuntime>, AppError> {
    let id = jar
        .get(JOB_COOKIE)
        .map(|cookie| cookie.value().to_owned())
        .ok_or_else(AppError::unauthorized)?;
    let owner = jar
        .get(OWNER_COOKIE)
        .map(|cookie| cookie.value().to_owned())
        .ok_or_else(AppError::unauthorized)?;
    state.manager.authorize(&id, &owner).await
}

fn private_cookie(
    name: &'static str,
    value: String,
    max_age: time::Duration,
    secure: bool,
) -> Cookie<'static> {
    Cookie::build((name, value))
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Strict)
        .max_age(max_age)
        .build()
}

fn clear_private_cookies(jar: CookieJar) -> CookieJar {
    jar.remove(Cookie::build(JOB_COOKIE).path("/").build())
        .remove(Cookie::build(OWNER_COOKIE).path("/").build())
}

fn upstream_error(error: String) -> AppError {
    let message: String = if error.contains("二维码")
        || error.contains("登录")
        || error.contains("QQ")
        || error.contains("xlogin")
    {
        error.chars().take(180).collect()
    } else {
        "QQ 登录服务暂时不可用，请稍后重试".into()
    };
    AppError::new(StatusCode::BAD_GATEWAY, "qq_login_failed", message)
}

async fn security_middleware(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) {
        if let Some(origin) = request
            .headers()
            .get(ORIGIN)
            .and_then(|value| value.to_str().ok())
        {
            if origin.trim_end_matches('/') != state.manager.config.public_origin {
                return AppError::new(
                    StatusCode::FORBIDDEN,
                    "origin_rejected",
                    "请求来源与本站不一致",
                )
                .into_response();
            }
        }
    }
    let is_api = request.uri().path().starts_with("/api/");
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    response
        .headers_mut()
        .insert("x-frame-options", HeaderValue::from_static("DENY"));
    response
        .headers_mut()
        .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    response.headers_mut().insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    response.headers_mut().insert(
        "content-security-policy",
        HeaderValue::from_static("default-src 'self'; base-uri 'self'; connect-src 'self'; form-action 'self'; frame-ancestors 'none'; img-src 'self' data: blob:; media-src 'self' blob:; object-src 'none'; script-src 'self'; style-src 'self'; worker-src 'self'"),
    );
    if is_api && !response.headers().contains_key(CACHE_CONTROL) {
        response.headers_mut().insert(
            CACHE_CONTROL,
            HeaderValue::from_static("no-store, max-age=0"),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use tempfile::tempdir;
    use tower::ServiceExt;

    use super::router;
    use crate::{config::Config, job::JobManager, model::JobPhase};

    #[tokio::test]
    async fn job_cookies_protect_status_from_other_owners() {
        let directory = tempdir().unwrap();
        let public = directory.path().join("public");
        std::fs::create_dir_all(&public).unwrap();
        std::fs::write(public.join("index.html"), "ok").unwrap();
        let manager = JobManager::new(Config::development(directory.path().join("jobs"), public))
            .await
            .unwrap();
        let app = router(manager);
        let response = app
            .clone()
            .oneshot(
                Request::post("/api/jobs")
                    .header("origin", "http://localhost")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let cookies = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .filter_map(|value| value.split(';').next())
            .collect::<Vec<_>>()
            .join("; ");
        let authorized = app
            .clone()
            .oneshot(
                Request::get("/api/job")
                    .header("cookie", &cookies)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(authorized.status(), StatusCode::OK);
        let wrong_owner = cookies
            .split("; ")
            .map(|cookie| {
                if cookie.starts_with("qzone_owner=") {
                    format!("qzone_owner={}", "0".repeat(64))
                } else {
                    cookie.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("; ");
        let denied = app
            .oneshot(
                Request::get("/api/job")
                    .header("cookie", wrong_owner)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn active_job_rejects_login_state_changes() {
        let directory = tempdir().unwrap();
        let public = directory.path().join("public");
        std::fs::create_dir_all(&public).unwrap();
        std::fs::write(public.join("index.html"), "ok").unwrap();
        let manager = JobManager::new(Config::development(directory.path().join("jobs"), public))
            .await
            .unwrap();
        let (job, owner) = manager.create().await.unwrap();
        job.update(|status| status.phase = JobPhase::Queued)
            .await
            .unwrap();
        let cookie = format!("qzone_job={}; qzone_owner={owner}", job.id);
        let app = router(manager);
        for path in ["/api/login/qr", "/api/login/poll"] {
            let response = app
                .clone()
                .oneshot(
                    Request::post(path)
                        .header("origin", "http://localhost")
                        .header("cookie", &cookie)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::CONFLICT);
        }
    }

    #[test]
    fn byte_ranges_cover_video_seeking_shapes() {
        assert_eq!(super::parse_byte_range(None, 10).unwrap(), None);
        assert_eq!(
            super::parse_byte_range(Some("bytes=2-5"), 10).unwrap(),
            Some((2, 5))
        );
        assert_eq!(
            super::parse_byte_range(Some("bytes=7-"), 10).unwrap(),
            Some((7, 9))
        );
        assert_eq!(
            super::parse_byte_range(Some("bytes=-3"), 10).unwrap(),
            Some((7, 9))
        );
        assert_eq!(
            super::parse_byte_range(Some("bytes=12-15"), 10)
                .unwrap_err()
                .status,
            StatusCode::RANGE_NOT_SATISFIABLE
        );
    }

    #[tokio::test]
    async fn ready_viewer_is_owner_only_and_supports_media_ranges() {
        let directory = tempdir().unwrap();
        let public = directory.path().join("public");
        std::fs::create_dir_all(&public).unwrap();
        std::fs::write(public.join("index.html"), "ok").unwrap();
        let manager = JobManager::new(Config::development(directory.path().join("jobs"), public))
            .await
            .unwrap();
        let (job, owner) = manager.create().await.unwrap();
        std::fs::create_dir_all(job.media_dir()).unwrap();
        std::fs::write(job.viewer_manifest_path(), r#"{"formatVersion":2}"#).unwrap();
        crate::database::initialize(&job.db_path()).unwrap();
        crate::database::replace_viewer_records(
            &job.db_path(),
            &[crate::database::ExportRecord {
                id: 1,
                cell_id: "cell-1".into(),
                published_at: 1_735_689_600,
                content: Some("雨声".into()),
                author_name: Some("故人".into()),
                category: "other".into(),
                media: vec!["media/clip.mp4".into()],
            }],
        )
        .unwrap();
        std::fs::write(job.media_dir().join("clip.mp4"), b"0123456789").unwrap();
        job.update(|status| {
            status.phase = JobPhase::Ready;
            status.download_ready = true;
        })
        .await
        .unwrap();
        let cookie = format!("qzone_job={}; qzone_owner={owner}", job.id);
        let app = router(manager);

        let denied = app
            .clone()
            .oneshot(
                Request::get("/api/archive/viewer/manifest")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);

        let manifest = app
            .clone()
            .oneshot(
                Request::get("/api/archive/viewer/manifest")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(manifest.status(), StatusCode::OK);
        assert_eq!(
            manifest.headers()["cache-control"],
            "private, no-store, max-age=0"
        );

        let records = app
            .clone()
            .oneshot(
                Request::get("/api/archive/viewer/records?limit=1&q=%E9%9B%A8%E5%A3%B0")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(records.status(), StatusCode::OK);
        let records = to_bytes(records.into_body(), 4096).await.unwrap();
        let records: serde_json::Value = serde_json::from_slice(&records).unwrap();
        assert_eq!(records["total"], 1);
        assert_eq!(records["items"][0]["content"], "雨声");

        let media = app
            .oneshot(
                Request::get("/api/archive/viewer/media/clip.mp4")
                    .header("cookie", &cookie)
                    .header("range", "bytes=2-5")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(media.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(media.headers()["content-range"], "bytes 2-5/10");
        assert_eq!(to_bytes(media.into_body(), 16).await.unwrap(), &b"2345"[..]);
    }
}
