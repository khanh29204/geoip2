use std::path::PathBuf;

use geoip_rust_service::{config::Config, models::GeoIpResponse};

#[test]
fn defaults_to_city_database_and_port_8080() {
    let config = Config::parse(None, None).expect("defaults should be valid");

    assert_eq!(config.db_path, PathBuf::from("/data/GeoLite2-City.mmdb"));
    assert_eq!(config.port, 8080);
}

#[test]
fn serializes_missing_response_fields_as_null() {
    let response = GeoIpResponse {
        ip: "192.0.2.1".into(),
        country_code: None,
        country_name: None,
        region_code: None,
        region_name: None,
        city: None,
        zip_code: None,
        time_zone: None,
        latitude: None,
        longitude: None,
    };
    let value = serde_json::to_value(response).expect("response should serialize");

    assert_eq!(value["ip"], "192.0.2.1");
    for field in [
        "country_code",
        "country_name",
        "region_code",
        "region_name",
        "city",
        "zip_code",
        "time_zone",
        "latitude",
        "longitude",
    ] {
        assert!(value[field].is_null(), "{field} should serialize as null");
    }
}

#[test]
fn rejects_a_non_numeric_port() {
    let result = Config::parse(None, Some("not-a-port".into()));

    assert!(result.is_err());
}

#[test]
fn rejects_an_empty_database_path() {
    let result = Config::parse(Some("".into()), None);

    assert!(result.is_err());
}

#[cfg(unix)]
#[test]
fn rejects_a_non_unicode_port() {
    use std::os::unix::ffi::OsStringExt;

    let result = Config::parse(None, Some(std::ffi::OsString::from_vec(vec![0xff])));

    assert!(result.is_err());
}
