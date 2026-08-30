use std::{convert::Infallible, sync::Arc};

use axum::{
    body::Body,
    extract::State,
    http::{
        header::{
            CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, ORIGIN,
            X_CONTENT_TYPE_OPTIONS,
        },
        HeaderValue, Method, Request, StatusCode,
    },
    middleware::{self, Next},
    response::{sse::Event, IntoResponse, Response, Sse},
    routing::{get, post},
    Json, Router,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use futures_util::{stream, Stream, StreamExt};
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream};
use tokio_util::io::ReaderStream;
use tower_http::{
    compression::CompressionLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

use crate::{
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
        HeaderValue::from_static("default-src 'self'; base-uri 'self'; connect-src 'self'; form-action 'self'; frame-ancestors 'none'; img-src 'self' data:; media-src 'self' blob:; object-src 'none'; script-src 'self'; style-src 'self'"),
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
        body::Body,
        http::{Request, StatusCode},
    };
    use tempfile::tempdir;
    use tower::ServiceExt;

    use super::router;
    use crate::{config::Config, job::JobManager};

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
}
