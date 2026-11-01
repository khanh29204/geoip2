use geoip_rust_service::{db::GeoDb, models::GeoIpResponse};
use std::{
    net::IpAddr,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    thread,
};

static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "geoip2-db-test-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn database(&self, name: &str, file: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, fixture_bytes(file)).unwrap();
        path
    }

    fn pair(&self, city: &str, asn: &str) -> (PathBuf, PathBuf) {
        (
            self.database("city.mmdb", city),
            self.database("asn.mmdb", asn),
        )
    }

    fn replace(&self, file: &str, path: &Path) {
        let replacement = self.database("replacement.mmdb", file);
        std::fs::rename(replacement, path).unwrap();
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture_bytes(name: &str) -> &'static [u8] {
    match name {
        "city" => include_bytes!("fixtures/GeoIP2-City-Test.mmdb"),
        "geolite" => include_bytes!("fixtures/GeoLite2-City-Test.mmdb"),
        "country" => include_bytes!("fixtures/GeoIP2-Country-Test.mmdb"),
        "asn" => include_bytes!("fixtures/GeoLite2-ASN-Test.mmdb"),
        "broken-double" => include_bytes!("fixtures/GeoIP2-City-Test-Broken-Double-Format.mmdb"),
        _ => panic!("unknown fixture"),
    }
}

fn lookup(db: &GeoDb, ip: &str) -> Result<Option<GeoIpResponse>, maxminddb::MaxMindDbError> {
    db.lookup(ip.parse::<IpAddr>().unwrap())
}

#[test]
fn merges_city_and_asn_records() {
    let temp = TempDir::new();
    let (city, asn) = temp.pair("city", "asn");
    let db = GeoDb::open(city, asn).unwrap();
    let response = lookup(&db, "89.160.20.128").unwrap().unwrap();

    assert_eq!(response.ip, "89.160.20.128");
    assert_eq!(response.country_code.as_deref(), Some("SE"));
    assert_eq!(response.country_name.as_deref(), Some("Sweden"));
    assert_eq!(response.region_code.as_deref(), Some("E"));
    assert_eq!(response.region_name.as_deref(), Some("Östergötland County"));
    assert_eq!(response.city.as_deref(), Some("Linköping"));
    assert_eq!(response.zip_code, None);
    assert_eq!(response.time_zone.as_deref(), Some("Europe/Stockholm"));
    assert_eq!(response.latitude, Some(58.4167));
    assert_eq!(response.longitude, Some(15.6167));
    assert_eq!(response.asn, Some(29518));
    assert_eq!(response.asn_organization.as_deref(), Some("Bredband2 AB"));
}

#[test]
fn accepts_geolite_city_and_combines_city_only_and_asn_only_ipv4_and_ipv6() {
    let temp = TempDir::new();
    let (city, asn) = temp.pair("geolite", "asn");
    let db = GeoDb::open(city, asn).unwrap();

    let city_only = lookup(&db, "2001:218::").unwrap().unwrap();
    assert_eq!(city_only.country_code.as_deref(), Some("JP"));
    assert_eq!(city_only.asn, None);
    assert_eq!(city_only.asn_organization, None);

    let asn_only = lookup(&db, "1.0.0.1").unwrap().unwrap();
    assert_eq!(
        (
            asn_only.country_code.as_deref(),
            asn_only.country_name.as_deref(),
            asn_only.region_code.as_deref(),
            asn_only.region_name.as_deref(),
            asn_only.city.as_deref(),
            asn_only.zip_code.as_deref(),
            asn_only.time_zone.as_deref(),
            asn_only.latitude,
            asn_only.longitude,
        ),
        (None, None, None, None, None, None, None, None, None)
    );
    assert_eq!(asn_only.asn, Some(15169));
    assert_eq!(asn_only.asn_organization.as_deref(), Some("Google Inc."));

    assert!(matches!(lookup(&db, "203.0.113.1"), Ok(None)));
    assert!(matches!(lookup(&db, "0.0.0.0"), Ok(None)));
    assert!(matches!(lookup(&db, "2001:db8::1"), Ok(None)));
}

#[test]
fn rejects_wrong_database_types_for_each_argument() {
    let temp = TempDir::new();
    let country = temp.database("country.mmdb", "country");
    let asn = temp.database("asn.mmdb", "asn");
    assert!(GeoDb::open(country, asn).is_err());

    let city = temp.database("city.mmdb", "city");
    let wrong_asn = temp.database("wrong-asn.mmdb", "city");
    assert!(GeoDb::open(city, wrong_asn).is_err());
}

#[test]
fn opens_broken_double_test_fixture_but_returns_decode_error_for_lookup() {
    let temp = TempDir::new();
    let city = temp.database("city.mmdb", "broken-double");
    let asn = temp.database("asn.mmdb", "asn");
    let db = GeoDb::open(city, asn).unwrap();
    assert!(matches!(
        lookup(&db, "2001:220::"),
        Err(maxminddb::MaxMindDbError::InvalidDatabase { .. })
    ));
}

#[test]
fn reload_reports_unchanged_then_swaps_atomic_city_and_asn_replacements() {
    let temp = TempDir::new();
    let (city, asn) = temp.pair("city", "asn");
    let db = GeoDb::open(&city, &asn).unwrap();
    assert!(!db.reload_if_changed().unwrap());
    assert!(lookup(&db, "214.0.0.1").unwrap().unwrap().city.is_some());
    assert!(lookup(&db, "214.78.0.1").unwrap().unwrap().city.is_none());
    assert_eq!(lookup(&db, "214.0.0.1").unwrap().unwrap().asn, Some(721));

    temp.replace("geolite", &city);
    assert!(db.reload_if_changed().unwrap());
    assert!(lookup(&db, "214.0.0.1").unwrap().unwrap().city.is_none());
    assert!(lookup(&db, "214.78.0.1").unwrap().unwrap().city.is_some());
    assert!(!db.reload_if_changed().unwrap());

    temp.replace("city", &city);
    temp.replace("asn", &asn);
    assert!(db.reload_if_changed().unwrap());
}

#[test]
fn reload_attempts_asn_when_city_fails_and_preserves_failed_city_reader() {
    let temp = TempDir::new();
    let (city, asn) = temp.pair("city", "asn");
    let db = GeoDb::open(&city, &asn).unwrap();

    let old_city = temp.path().join("old-city.mmdb");
    std::fs::rename(&city, &old_city).unwrap();
    let corrupt = temp.path().join("corrupt.mmdb");
    std::fs::write(&corrupt, b"not a MaxMind database").unwrap();
    std::fs::rename(corrupt, &city).unwrap();
    temp.replace("asn", &asn);
    assert!(db.reload_if_changed().is_err());
    assert!(lookup(&db, "89.160.20.128")
        .unwrap()
        .unwrap()
        .city
        .is_some());

    std::fs::rename(old_city, &city).unwrap();
    assert!(!db.reload_if_changed().unwrap());
}

#[test]
fn reload_attempts_city_when_asn_fails_and_preserves_failed_asn_reader() {
    let temp = TempDir::new();
    let (city, asn) = temp.pair("city", "asn");
    let db = GeoDb::open(&city, &asn).unwrap();

    temp.replace("geolite", &city);
    let corrupt = temp.path().join("corrupt.mmdb");
    std::fs::write(&corrupt, b"not a MaxMind database").unwrap();
    std::fs::rename(corrupt, &asn).unwrap();
    assert!(db.reload_if_changed().is_err());
    let reloaded_city = lookup(&db, "214.78.0.1").unwrap().unwrap();
    assert!(reloaded_city.city.is_some());
    assert_eq!(reloaded_city.asn, Some(721));

    temp.replace("asn", &asn);
    assert!(db.reload_if_changed().unwrap());
    assert_eq!(
        lookup(&db, "89.160.20.128").unwrap().unwrap().asn,
        Some(29518)
    );
}

#[test]
fn lookups_remain_valid_while_database_snapshots_are_replaced() {
    let temp = TempDir::new();
    let (city, asn) = temp.pair("city", "asn");
    let db = Arc::new(GeoDb::open(&city, &asn).unwrap());
    let running = Arc::new(AtomicBool::new(true));
    let lookup_db = Arc::clone(&db);
    let lookup_running = Arc::clone(&running);
    let reader_thread = thread::spawn(move || {
        while lookup_running.load(Ordering::Relaxed) {
            let result = lookup(&lookup_db, "89.160.20.128").unwrap().unwrap();
            assert_eq!(result.country_code.as_deref(), Some("SE"));
            assert_eq!(result.asn, Some(29518));
        }
    });

    for index in 0..20 {
        let source = if index % 2 == 0 { "geolite" } else { "city" };
        temp.replace(source, &city);
        assert!(db.reload_if_changed().unwrap());
    }
    running.store(false, Ordering::Relaxed);
    reader_thread.join().unwrap();
}
