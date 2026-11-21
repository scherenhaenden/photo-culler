use std::{
    fs,
    net::SocketAddr,
    path::PathBuf,
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    process::{Child, Command},
    time::timeout,
};

const THUMBNAIL: &[u8] =
    include_bytes!("../../../../migration/demo/fixture/demo-001-thumbnail.jpg");

fn server_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_photo-culler-server"));
    command.kill_on_drop(true);
    command
}

async fn start_owned(
    extra_args: &[&str],
) -> (Child, BufReader<tokio::process::ChildStdout>, String) {
    let mut child = server_command()
        .arg("--owned")
        .arg("--port")
        .arg("0")
        .args(extra_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    let mut stdout = stdout;
    let mut line = String::new();
    timeout(Duration::from_secs(5), stdout.read_line(&mut line))
        .await
        .expect("owned server should announce readiness")
        .unwrap();
    assert!(line.ends_with('\n'));
    (child, stdout, line)
}

async fn request(address: SocketAddr, path: &str) -> Vec<u8> {
    request_with_token(address, path, None).await
}

async fn request_with_token(address: SocketAddr, path: &str, token: Option<&str>) -> Vec<u8> {
    let mut stream = TcpStream::connect(address).await.unwrap();
    let authorization = token
        .map(|token| format!("Authorization: Bearer {token}\r\n"))
        .unwrap_or_default();
    stream
        .write_all(
            format!(
                "GET {path} HTTP/1.1\r\nHost: {address}\r\n{authorization}Connection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    response
}

fn split_response(response: &[u8]) -> (&str, &[u8]) {
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("HTTP response header terminator");
    (
        std::str::from_utf8(&response[..split]).unwrap(),
        &response[split + 4..],
    )
}

fn temp_web_root() -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("photo-culler-s01-web-{suffix}"));
    fs::create_dir_all(root.join("assets")).unwrap();
    for (name, bytes) in [
        ("index.html", b"<h1>demo</h1>".as_slice()),
        ("bootstrap.js", b"bootstrap".as_slice()),
        ("photo_culler_slint.js", b"generated".as_slice()),
        ("photo_culler_slint_bg.wasm", b"\0asm\x01\0\0\0".as_slice()),
    ] {
        fs::write(root.join(name), bytes).unwrap();
    }
    fs::write(root.join("assets/icon.svg"), b"<svg/>").unwrap();
    fs::write(
        root.join("bundle.json"),
        br#"{"schema_version":1,"files":["index.html","bootstrap.js","photo_culler_slint.js","photo_culler_slint_bg.wasm","assets/icon.svg"]}"#,
    )
    .unwrap();
    root
}

#[tokio::test]
async fn owned_server_serves_health_gallery_and_bundled_thumbnail_then_stops_on_eof() {
    let (mut child, mut stdout, ready_line) = start_owned(&[]).await;
    let ready: serde_json::Value = serde_json::from_str(ready_line.trim()).unwrap();
    assert_eq!(ready["event"], "ready");
    assert_eq!(ready["protocol"], 1);
    let token = ready["session_token"].as_str().unwrap();
    let origin = ready["origin"].as_str().unwrap();
    let address: SocketAddr = origin.strip_prefix("http://").unwrap().parse().unwrap();
    assert_eq!(address.ip().to_string(), "127.0.0.1");
    assert_ne!(address.port(), 0);

    let health = request(address, "/api/v1/health").await;
    let (headers, body) = split_response(&health);
    assert!(headers.starts_with("HTTP/1.1 200"));
    assert!(
        headers
            .to_ascii_lowercase()
            .contains("content-type: application/json")
    );
    let health: serde_json::Value = serde_json::from_slice(body).unwrap();
    assert_eq!(health["status"], "ready");
    assert!(health["resources"].is_null());

    let gallery = request_with_token(address, "/api/v1/galleries/demo/photos", Some(token)).await;
    let (headers, body) = split_response(&gallery);
    assert!(headers.starts_with("HTTP/1.1 200"));
    let gallery: serde_json::Value = serde_json::from_slice(body).unwrap();
    assert_eq!(gallery["gallery_id"], "demo");
    assert_eq!(gallery["items"][0]["photo_id"], "demo-001");
    assert_eq!(
        gallery["items"][0]["thumbnail_url"],
        "/api/v1/photos/demo-001/thumbnail/800"
    );

    let thumbnail = request_with_token(
        address,
        "/api/v1/photos/demo-001/thumbnail/800",
        Some(token),
    )
    .await;
    let (headers, body) = split_response(&thumbnail);
    assert!(headers.starts_with("HTTP/1.1 200"));
    assert!(
        headers
            .to_ascii_lowercase()
            .contains("content-type: image/jpeg")
    );
    assert_eq!(body, THUMBNAIL);

    drop(child.stdin.take());
    let status = timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("EOF should stop an owned server")
        .unwrap();
    assert!(status.success());
    let mut extra_output = String::new();
    stdout.read_to_string(&mut extra_output).await.unwrap();
    assert!(
        extra_output.is_empty(),
        "owned stdout must contain only one ready line"
    );
    assert!(TcpStream::connect(address).await.is_err());
}

#[tokio::test]
async fn same_listener_serves_only_manifest_assets_with_their_mime_types() {
    let root = temp_web_root();
    let root_arg = root.to_string_lossy().into_owned();
    let (mut child, mut stdout, ready_line) = start_owned(&["--web-root", &root_arg]).await;
    let ready: serde_json::Value = serde_json::from_str(ready_line.trim()).unwrap();
    let address: SocketAddr = ready["origin"]
        .as_str()
        .unwrap()
        .strip_prefix("http://")
        .unwrap()
        .parse()
        .unwrap();

    for (path, mime, body) in [
        ("/", "text/html; charset=utf-8", b"<h1>demo</h1>".as_slice()),
        (
            "/bootstrap.js",
            "text/javascript; charset=utf-8",
            b"bootstrap".as_slice(),
        ),
        (
            "/photo_culler_slint_bg.wasm",
            "application/wasm",
            b"\0asm\x01\0\0\0".as_slice(),
        ),
        ("/assets/icon.svg", "image/svg+xml", b"<svg/>".as_slice()),
    ] {
        let response = request(address, path).await;
        let (headers, actual_body) = split_response(&response);
        assert!(headers.starts_with("HTTP/1.1 200"), "{path}: {headers}");
        assert!(
            headers
                .to_ascii_lowercase()
                .contains(&format!("content-type: {mime}"))
        );
        assert_eq!(actual_body, body);
    }
    let manifest = request(address, "/bundle.json").await;
    assert!(split_response(&manifest).0.starts_with("HTTP/1.1 404"));
    let undeclared = request(address, "/secret.txt").await;
    assert!(split_response(&undeclared).0.starts_with("HTTP/1.1 404"));

    drop(child.stdin.take());
    assert!(
        timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    let mut extra_output = String::new();
    stdout.read_to_string(&mut extra_output).await.unwrap();
    assert!(extra_output.is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn invalid_web_root_fails_before_owned_ready_announcement() {
    let missing =
        std::env::temp_dir().join(format!("photo-culler-s01-missing-{}", std::process::id()));
    let child = server_command()
        .args(["--owned", "--port", "0", "--web-root"])
        .arg(missing)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let output = timeout(Duration::from_secs(5), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid web bundle"));
}

#[cfg(unix)]
#[tokio::test]
async fn standalone_server_stops_on_sigint() {
    let mut child = server_command()
        .arg("--port")
        .arg("0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut origin = String::new();
    timeout(Duration::from_secs(5), stdout.read_line(&mut origin))
        .await
        .unwrap()
        .unwrap();
    let address: SocketAddr = origin
        .trim()
        .strip_prefix("http://")
        .unwrap()
        .parse()
        .unwrap();
    assert_ne!(address.port(), 0);

    let pid = child.id().unwrap().to_string();
    let signal = std::process::Command::new("kill")
        .args(["-INT", &pid])
        .status()
        .unwrap();
    assert!(signal.success());
    let status = timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("Ctrl+C should stop the standalone server")
        .unwrap();
    assert!(status.success());
    assert!(TcpStream::connect(address).await.is_err());
}
