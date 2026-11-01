use std::{net::IpAddr, sync::Arc};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde_json::json;

use crate::db::GeoDb;

pub fn router(db: Arc<GeoDb>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/json/{ip}", get(lookup))
        .route("/api/v1/geoip/{ip}", get(lookup))
        .fallback(not_found)
        .with_state(db)
}

async fn health() -> Response {
    (StatusCode::OK, Json(json!({"status": "ok"}))).into_response()
}

async fn lookup(State(db): State<Arc<GeoDb>>, Path(ip): Path<String>) -> Response {
    let Ok(ip) = ip.parse::<IpAddr>() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid IP address"})),
        )
            .into_response();
    };

    match db.lookup(ip) {
        Ok(Some(response)) => Json(response).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "IP address not found"})),
        )
            .into_response(),
        Err(error) => {
            tracing::error!(error = ?error, "GeoIP database lookup failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "internal server error"})),
            )
                .into_response()
        }
    }
}

async fn not_found() -> Response {
    (StatusCode::NOT_FOUND, Json(json!({"error": "not found"}))).into_response()
}
