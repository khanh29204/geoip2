use std::{
    env, fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, ExitStatus},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new() -> Self {
        let root = env::var_os("TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(env::temp_dir);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        let path = root.join(format!("geoip-startup-{}-{unique}", std::process::id()));
        fs::create_dir_all(&path).expect("scratch directory should be created");
        Self(path)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Service(Child);

impl Drop for Service {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("ephemeral port should be available")
        .local_addr()
        .expect("listener should have an address")
        .port()
}

fn http_status(port: u16) -> Option<String> {
    let mut stream = TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().ok()?,
        Duration::from_millis(200),
    )
    .ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(1))).ok()?;
    stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    response.lines().next().map(str::to_owned)
}

fn stop(service: &mut Service) -> ExitStatus {
    #[cfg(unix)]
    {
        let status = Command::new("kill")
            .args(["-TERM", &service.0.id().to_string()])
            .status()
            .expect("kill command should send SIGTERM");
        assert!(status.success(), "kill -TERM should succeed");
    }
    #[cfg(not(unix))]
    service.0.kill().expect("service should be stopped");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = service
            .0
            .try_wait()
            .expect("child status should be readable")
        {
            return status;
        }
        assert!(Instant::now() < deadline, "service did not stop promptly");
        thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn waits_for_database_install_and_serves_health_then_stops_on_sigterm() {
    let scratch = ScratchDir::new();
    let database = scratch.0.join("GeoIP2-City-Test.mmdb");
    let asn_database = scratch.0.join("GeoLite2-ASN-Test.mmdb");
    install_fixture("GeoIP2-City-Test.mmdb", &database, &scratch.0);
    let port = free_port();
    let mut service = Service(
        Command::new(env!("CARGO_BIN_EXE_geoip-rust-service"))
            .env("DB_PATH", &database)
            .env("ASN_DB_PATH", &asn_database)
            .env("PORT", port.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("service binary should start"),
    );

    // City is available, but startup must still wait for the ASN database.
    thread::sleep(Duration::from_millis(500));
    assert!(service
        .0
        .try_wait()
        .expect("child status should be readable")
        .is_none());
    assert!(
        http_status(port).is_none(),
        "service started without ASN data"
    );
    install_fixture("GeoLite2-ASN-Test.mmdb", &asn_database, &scratch.0);

    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = http_status(port) {
            break status;
        }
        assert!(Instant::now() < deadline, "service never served /health");
        assert!(service
            .0
            .try_wait()
            .expect("child status should be readable")
            .is_none());
        thread::sleep(Duration::from_millis(50));
    };
    assert!(
        status.starts_with("HTTP/1.1 200"),
        "unexpected response: {status}"
    );

    let exit = stop(&mut service);
    #[cfg(unix)]
    assert!(
        exit.success(),
        "service should exit cleanly on SIGTERM: {exit}"
    );
    #[cfg(not(unix))]
    assert!(!exit.success(), "forced stop should terminate the process");
}

#[test]
fn sigterm_stops_promptly_while_database_is_missing() {
    let scratch = ScratchDir::new();
    let mut service = Service(
        Command::new(env!("CARGO_BIN_EXE_geoip-rust-service"))
            .env("DB_PATH", scratch.0.join("missing.mmdb"))
            .env("ASN_DB_PATH", scratch.0.join("missing-asn.mmdb"))
            .env("PORT", "0")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("service binary should start"),
    );
    thread::sleep(Duration::from_millis(500));
    assert!(service
        .0
        .try_wait()
        .expect("child status should be readable")
        .is_none());

    let started = Instant::now();
    let exit = stop(&mut service);
    assert!(
        exit.success(),
        "service should exit cleanly on SIGTERM: {exit}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "shutdown was not prompt"
    );
}

#[test]
fn invalid_port_exits_nonzero_without_waiting_for_database() {
    let scratch = ScratchDir::new();
    let output = Command::new(env!("CARGO_BIN_EXE_geoip-rust-service"))
        .env("DB_PATH", scratch.0.join("missing.mmdb"))
        .env("ASN_DB_PATH", scratch.0.join("missing-asn.mmdb"))
        .env("PORT", "not-a-port")
        .output()
        .expect("service binary should start");

    assert!(!output.status.success(), "invalid PORT should fail startup");
}

fn install_fixture(name: &str, target: &PathBuf, scratch: &std::path::Path) {
    let staged = scratch.join(format!("{name}.new"));
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
        &staged,
    )
    .expect("fixture should be copied");
    fs::rename(staged, target).expect("database should be atomically installed");
}
