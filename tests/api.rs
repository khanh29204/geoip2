use std::{collections::BTreeSet, path::PathBuf, sync::Arc};

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use tower::ServiceExt;

use geoip_rust_service::{db::GeoDb, handlers};

async fn get(uri: &str) -> (StatusCode, Value) {
    let db = GeoDb::open(PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/GeoIP2-City-Test.mmdb"
    )))
    .expect("open test database");
    let response = handlers::router(Arc::new(db))
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

#[tokio::test]
async fn health_is_public_and_returns_ok() {
    let (status, body) = get("/health").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"status": "ok"}));
}

#[tokio::test]
async fn city_record_has_the_exact_json_fields_and_official_values() {
    let (status, body) = get("/json/89.160.20.128").await;

    assert_eq!(status, StatusCode::OK);
    let keys: BTreeSet<_> = body
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "ip",
            "country_code",
            "country_name",
            "region_code",
            "region_name",
            "city",
            "zip_code",
            "time_zone",
            "latitude",
            "longitude",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(body["ip"], "89.160.20.128");
    assert_eq!(body["country_code"], "SE");
    assert_eq!(body["country_name"], "Sweden");
    assert_eq!(body["region_code"], "E");
    assert_eq!(body["region_name"], "Östergötland County");
    assert_eq!(body["city"], "Linköping");
    assert!(body["zip_code"].is_null());
    assert_eq!(body["time_zone"], "Europe/Stockholm");
    assert_eq!(body["latitude"], 58.4167);
    assert_eq!(body["longitude"], 15.6167);
}

#[tokio::test]
async fn versioned_route_is_an_alias_of_json_route() {
    let (alias_status, alias) = get("/api/v1/geoip/89.160.20.128").await;
    let (json_status, json) = get("/json/89.160.20.128").await;

    assert_eq!(alias_status, StatusCode::OK);
    assert_eq!(alias_status, json_status);
    assert_eq!(alias, json);
}

#[tokio::test]
async fn invalid_ip_returns_the_public_400_error() {
    let (status, body) = get("/json/not-an-ip").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body, json!({"error": "invalid IP address"}));
}

#[tokio::test]
async fn missing_routes_return_json_404() {
    for uri in ["/json/", "/api/v1/geoip/"] {
        let (status, body) = get(uri).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, json!({"error": "not found"}));
    }
}

#[tokio::test]
async fn address_absent_from_database_returns_non_sensitive_404() {
    let (status, body) = get("/json/203.0.113.1").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, json!({"error": "IP address not found"}));
}

#[tokio::test]
async fn known_ipv6_address_uses_the_same_lookup_route() {
    let (status, body) = get("/json/2001:218::").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ip"], "2001:218::");
    assert_eq!(body["country_code"], "JP");
}

#[tokio::test]
async fn invalid_database_lookup_returns_non_sensitive_500() {
    let db = GeoDb::open(PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/GeoIP2-City-Test-Broken-Double-Format.mmdb"
    )))
    .expect("open broken test database");
    let response = handlers::router(Arc::new(db))
        .oneshot(
            Request::builder()
                .uri("/json/89.160.20.128")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        json!({"error": "internal server error"})
    );
}
