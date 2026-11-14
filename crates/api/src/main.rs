use anyhow::Context;
use nullbox_api::{AppState, Config, db, http, observability};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=debug".into());
    if std::env::var("LOG_FORMAT").is_ok_and(|v| v.eq_ignore_ascii_case("pretty")) {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).event_format(observability::JsonLogFormatter).init();
    }
    std::panic::set_hook(Box::new(|info| {
        tracing::error!(target: "EXISTENTIAL", %info, "the server has stared into the void, and the void panicked back");
    }));

    let config = Config::from_env().context("reading config")?;
    if config.admin_token.is_none() {
        tracing::warn!("ADMIN_TOKEN is not set: /api/admin/* refuses every request");
    }
    let pool = db::connect(&config.database_url).await.context("opening the database")?;
    let bind_addr = config.bind_addr;
    let app = http::router(AppState::new(pool, config));

    let listener = tokio::net::TcpListener::bind(bind_addr).await.with_context(|| format!("binding {bind_addr}"))?;
    tracing::info!(%bind_addr, "Nullbox is listening. The void awaits.");
    // The peer address is what the rate limiter and the audit log key on.
    axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>())
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
            tracing::info!("shutting down, tickets will have to wait");
        })
        .await?;
    Ok(())
}
