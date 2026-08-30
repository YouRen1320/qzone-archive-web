mod api;
mod archive;
mod config;
mod database;
mod error;
mod export;
mod job;
mod model;
mod qq;
mod tokens;

use std::process::ExitCode;

use config::Config;
use job::JobManager;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("qzone_archive_web=info,tower_http=info")),
        )
        .compact()
        .init();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(code = error.code, message = %error.message, "service stopped");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), error::AppError> {
    let config = Config::from_env()?;
    let bind = config.bind;
    let manager = JobManager::new(config).await?;
    manager.spawn_cleanup();
    let app = api::router(manager);
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|_| error::AppError::configuration("无法监听配置的服务端口"))?;
    tracing::info!(address = %listener.local_addr().unwrap_or(bind), "qzone archive web started");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|_| error::AppError::internal("HTTP 服务意外停止"))?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
