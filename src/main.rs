use std::{error::Error, io, sync::Arc, time::Duration};

use geoip_rust_service::{config::Config, db::GeoDb, handlers};
use tokio::time::sleep;

#[tokio::main(worker_threads = 2)]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        .init();

    let config = Config::from_env()?;
    let mut shutdown = Box::pin(shutdown_signal());

    let db = loop {
        let city_path = config.db_path.clone();
        let asn_path = config.asn_db_path.clone();
        match tokio::task::spawn_blocking(move || GeoDb::open(city_path, asn_path)).await? {
            Ok(db) => break Arc::new(db),
            Err(error) if is_not_found(error.as_ref()) => {
                tracing::warn!(
                    city_path = %config.db_path.display(),
                    asn_path = %config.asn_db_path.display(),
                    error = %error,
                    "GeoIP database is missing; retrying in 5 seconds"
                );
                tokio::select! {
                    _ = &mut shutdown => {
                        tracing::info!("shutdown requested while waiting for the GeoIP database");
                        return Ok(());
                    }
                    _ = sleep(Duration::from_secs(5)) => {}
                }
            }
            Err(error) => {
                tracing::error!(
                    city_path = %config.db_path.display(),
                    asn_path = %config.asn_db_path.display(),
                    error = %error,
                    "failed to open GeoIP databases"
                );
                return Err(error);
            }
        }
    };

    let address = std::net::SocketAddr::from(([0, 0, 0, 0], config.port));
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!(address = %address, "GeoIP service listening");

    let watcher = tokio::spawn({
        let db = Arc::clone(&db);
        async move {
            loop {
                sleep(Duration::from_secs(5)).await;
                let db = Arc::clone(&db);
                match tokio::task::spawn_blocking(move || db.reload_if_changed()).await {
                    Ok(Ok(true)) => tracing::info!("GeoIP database reloaded"),
                    Ok(Ok(false)) => {}
                    Ok(Err(error)) => {
                        tracing::warn!(error = %error, "GeoIP database reload failed; failed readers retained, other databases may have updated")
                    }
                    Err(error) => {
                        tracing::warn!(error = %error, "GeoIP database reload task failed")
                    }
                }
            }
        }
    });

    let result = axum::serve(listener, handlers::router(db))
        .with_graceful_shutdown(shutdown)
        .await;
    watcher.abort();
    let _ = watcher.await;
    result?;
    Ok(())
}

fn is_not_found(error: &(dyn Error + 'static)) -> bool {
    if error
        .downcast_ref::<io::Error>()
        .is_some_and(|error| error.kind() == io::ErrorKind::NotFound)
    {
        return true;
    }
    error.source().is_some_and(is_not_found)
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(signal) => signal,
                Err(error) => {
                    tracing::error!(error = %error, "failed to register SIGTERM handler");
                    return;
                }
            };
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                if let Err(error) = result {
                    tracing::error!(error = %error, "failed to listen for SIGINT");
                }
            }
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(error = %error, "failed to listen for Ctrl-C");
    }
    tracing::info!("shutdown signal received");
}
