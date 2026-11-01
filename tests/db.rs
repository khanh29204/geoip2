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
        "broken-double" => include_bytes!("fixtures/GeoIP2-City-Test-Broken-Double-Format.mmdb"),
        _ => panic!("unknown fixture"),
    }
}

fn lookup(db: &GeoDb, ip: &str) -> Result<Option<GeoIpResponse>, maxminddb::MaxMindDbError> {
    db.lookup(ip.parse::<IpAddr>().unwrap())
}

#[test]
fn opens_city_database_and_looks_up_fixture_record() {
    let temp = TempDir::new();
    let path = temp.database("city.mmdb", "city");
    let db = GeoDb::open(&path).unwrap();
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
}

#[test]
fn accepts_geolite_city_and_returns_none_for_missing_ipv4_and_ipv6() {
    let temp = TempDir::new();
    let path = temp.database("city.mmdb", "geolite");
    let db = GeoDb::open(path).unwrap();
    let known = lookup(&db, "89.160.20.128").unwrap().unwrap();
    assert_eq!(known.country_code.as_deref(), Some("SE"));

    assert!(matches!(lookup(&db, "0.0.0.0"), Ok(None)));
    assert!(matches!(lookup(&db, "2001:db8::1"), Ok(None)));
}

#[test]
fn rejects_country_database() {
    let temp = TempDir::new();
    let path = temp.database("country.mmdb", "country");
    assert!(GeoDb::open(path).is_err());
}

#[test]
fn opens_broken_double_test_fixture_but_returns_decode_error_for_lookup() {
    let temp = TempDir::new();
    let path = temp.database("broken.mmdb", "broken-double");
    let db = GeoDb::open(path).unwrap();
    assert!(matches!(
        lookup(&db, "2001:220::"),
        Err(maxminddb::MaxMindDbError::InvalidDatabase { .. })
    ));
}

#[test]
fn reload_reports_unchanged_then_swaps_an_atomic_replacement() {
    let temp = TempDir::new();
    let path = temp.database("active.mmdb", "city");
    let db = GeoDb::open(&path).unwrap();
    assert!(!db.reload_if_changed().unwrap());
    assert!(lookup(&db, "214.0.0.1").unwrap().is_some());
    assert!(matches!(lookup(&db, "214.78.0.1"), Ok(None)));

    let replacement = temp.database("replacement.mmdb", "geolite");
    std::fs::rename(&replacement, &path).unwrap();
    assert!(db.reload_if_changed().unwrap());
    assert!(matches!(lookup(&db, "214.0.0.1"), Ok(None)));
    assert!(lookup(&db, "214.78.0.1").unwrap().is_some());
    assert!(!db.reload_if_changed().unwrap());
}

#[test]
fn failed_reload_keeps_old_reader_and_retries_after_a_valid_replacement() {
    let temp = TempDir::new();
    let path = temp.database("active.mmdb", "city");
    let db = GeoDb::open(&path).unwrap();

    let corrupt = temp.path().join("corrupt.mmdb");
    std::fs::write(&corrupt, b"not a MaxMind database").unwrap();
    std::fs::rename(&corrupt, &path).unwrap();
    assert!(db.reload_if_changed().is_err());
    assert!(lookup(&db, "89.160.20.128").unwrap().is_some());
    assert!(lookup(&db, "214.0.0.1").unwrap().is_some());

    let replacement = temp.database("replacement.mmdb", "geolite");
    std::fs::rename(&replacement, &path).unwrap();
    assert!(db.reload_if_changed().unwrap());
    assert!(lookup(&db, "214.78.0.1").unwrap().is_some());
}

#[test]
fn lookups_remain_valid_while_database_snapshots_are_replaced() {
    let temp = TempDir::new();
    let path = temp.database("active.mmdb", "city");
    let db = Arc::new(GeoDb::open(&path).unwrap());
    let running = Arc::new(AtomicBool::new(true));
    let lookup_db = Arc::clone(&db);
    let lookup_running = Arc::clone(&running);
    let reader_thread = thread::spawn(move || {
        while lookup_running.load(Ordering::Relaxed) {
            let result = lookup(&lookup_db, "89.160.20.128").unwrap().unwrap();
            assert_eq!(result.country_code.as_deref(), Some("SE"));
        }
    });

    for index in 0..20 {
        let source = if index % 2 == 0 { "geolite" } else { "city" };
        let replacement = temp.database("replacement.mmdb", source);
        std::fs::rename(&replacement, &path).unwrap();
        assert!(db.reload_if_changed().unwrap());
    }
    running.store(false, Ordering::Relaxed);
    reader_thread.join().unwrap();
}
