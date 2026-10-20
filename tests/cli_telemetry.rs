//! `skillctl telemetry`: the local collector receives OTLP/JSON from a real
//! process, scrubs it, and `report`, `status`, `enable`, `disable` and
//! `doctor` read it back. HOME is redirected so the user's settings and
//! LaunchAgents are never touched, and `enable` runs with `--no-service`.
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::Path,
    process::{Child, Command, Stdio},
};

fn run(home: &Path, user_home: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", home)
        .env("HOME", user_home)
        .env_remove("CLAUDE_CODE_ENABLE_TELEMETRY")
        .env_remove("OTEL_EXPORTER_OTLP_ENDPOINT")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

struct Collector(Child);

impl Drop for Collector {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Start `telemetry serve --port 0` and read the bound port from its first line.
fn start(home: &Path, user_home: &Path) -> (Collector, u16) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", home)
        .env("HOME", user_home)
        .args(["telemetry", "serve", "--port", "0"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stderr.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let port = line
        .split("http://127.0.0.1:")
        .nth(1)
        .and_then(|rest| rest.split(';').next())
        .and_then(|port| port.parse().ok())
        .unwrap_or_else(|| panic!("no port in {line:?}"));
    (Collector(child), port)
}

/// Send requests over one keep-alive connection; returns each status line.
fn post(port: u16, requests: &[(&str, &str, Value)]) -> Vec<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut statuses = Vec::new();
    for (path, content_type, body) in requests {
        let body = body.to_string();
        write!(
            stream,
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut status = String::new();
        reader.read_line(&mut status).unwrap();
        let mut length = 0;
        loop {
            let mut header = String::new();
            reader.read_line(&mut header).unwrap();
            if header.trim().is_empty() {
                break;
            }
            if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse().unwrap();
            }
        }
        let mut response = vec![0; length];
        reader.read_exact(&mut response).unwrap();
        statuses.push(status.trim().to_owned());
    }
    statuses
}

fn kv(key: &str, value: Value) -> Value {
    json!({"key": key, "value": value})
}

fn now_nanos() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        .to_string()
}

#[test]
fn the_collector_records_scrubbed_events_that_report_status_and_doctor_read() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    let user_home = temp.path().join("user");
    fs::create_dir_all(&user_home).unwrap();
    let (_collector, port) = start(&home, &user_home);
    let at = now_nanos();
    let logs = json!({"resourceLogs": [{
        "resource": {"attributes": [kv("user.email", json!({"stringValue": "someone@example.com"}))]},
        "scopeLogs": [{"logRecords": [
            {"timeUnixNano": at, "attributes": [
                kv("event.name", json!({"stringValue": "api_request"})),
                kv("model", json!({"stringValue": "claude-sonnet-5"})),
                kv("cost_usd", json!({"doubleValue": 0.5})),
                kv("output_tokens", json!({"intValue": "1200"})),
                kv("skill.name", json!({"stringValue": "skill-check"}))
            ]},
            {"timeUnixNano": at, "attributes": [
                kv("event.name", json!({"stringValue": "user_prompt"})),
                kv("prompt", json!({"stringValue": "private words"}))
            ]}
        ]}]
    }]});
    let metrics = json!({"resourceMetrics": [{"scopeMetrics": [{"metrics": [
        {"name": "claude_code.active_time.total", "sum": {"aggregationTemporality": 1, "dataPoints": [
            {"timeUnixNano": at, "asDouble": 30.0, "attributes": [kv("type", json!({"stringValue": "cli"}))]}
        ]}}
    ]}]}]});
    let statuses = post(
        port,
        &[
            ("/v1/logs", "application/json", logs),
            ("/v1/metrics", "application/json", metrics),
            ("/v1/logs", "application/x-protobuf", json!({})),
            ("/v1/nope", "application/json", json!({})),
        ],
    );
    assert_eq!(
        statuses,
        [
            "HTTP/1.1 200 OK",
            "HTTP/1.1 200 OK",
            "HTTP/1.1 415 Unsupported Media Type",
            "HTTP/1.1 404 Not Found"
        ]
    );

    // Nothing private reaches the disk.
    let files: Vec<_> = fs::read_dir(home.join("telemetry"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    let written = fs::read_to_string(&files[0]).unwrap();
    assert_eq!(written.lines().count(), 3);
    assert!(!written.contains("someone@example.com"));
    assert!(!written.contains("private words"));

    let out = run(
        &home,
        &user_home,
        &["--json", "telemetry", "report", "--days", "1"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["source"], "logs");
    assert_eq!(report["total"]["requests"], 1);
    assert_eq!(report["total"]["cost"], 0.5);
    assert_eq!(report["by_skill"][0]["key"], "skill-check");
    assert_eq!(report["by_model"][0]["key"], "claude-sonnet-5");
    assert_eq!(report["active_seconds"]["cli"], 30.0);

    let port_text = port.to_string();
    let out = run(
        &home,
        &user_home,
        &["--json", "telemetry", "status", "--port", &port_text],
    );
    let status: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(status["collector"], "running");
    assert_eq!(status["settings"]["configured"], false);
    assert_eq!(status["service"]["installed"], false);
    assert_eq!(status["records_today"], 3);

    // enable merges into an existing env block and leaves other keys alone.
    let settings = user_home.join(".claude/settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(
        &settings,
        r#"{"env": {"CLAUDE_CODE_SUBAGENT_MODEL": "sonnet"}, "model": "opus"}"#,
    )
    .unwrap();
    let out = run(
        &home,
        &user_home,
        &[
            "--json",
            "telemetry",
            "enable",
            "--no-service",
            "--port",
            &port_text,
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let enabled: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(enabled["collector"], "running");
    assert_eq!(enabled["changed"].as_array().unwrap().len(), 6);
    let written: Value = serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(written["env"]["CLAUDE_CODE_SUBAGENT_MODEL"], "sonnet");
    assert_eq!(written["env"]["OTEL_EXPORTER_OTLP_PROTOCOL"], "http/json");
    assert_eq!(
        written["env"]["OTEL_EXPORTER_OTLP_ENDPOINT"],
        format!("http://127.0.0.1:{port}")
    );
    assert_eq!(written["model"], "opus");
    assert!(user_home.join(".claude/settings.json.bak").exists());

    // doctor reads the settings env block and sees the collector running.
    let out = run(&home, &user_home, &["--json", "doctor"]);
    let doctor: Value = serde_json::from_slice(&out.stdout).unwrap();
    let telemetry = &doctor["telemetry"];
    let checks: Vec<&str> = telemetry["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| finding["check"].as_str().unwrap())
        .collect();
    assert!(checks.contains(&"telemetry-collector"), "{checks:?}");
    assert!(!checks.contains(&"telemetry-off"), "{checks:?}");

    let out = run(
        &home,
        &user_home,
        &["--json", "telemetry", "disable", "--port", &port_text],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let written: Value = serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(
        written["env"],
        json!({"CLAUDE_CODE_SUBAGENT_MODEL": "sonnet"})
    );
}

#[test]
fn doctor_warns_when_the_configured_local_endpoint_has_no_listener() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    let user_home = temp.path().join("user");
    let settings = user_home.join(".claude/settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    // A port nothing listens on: bind, read it, release it.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let out = run(
        &home,
        &user_home,
        &[
            "telemetry",
            "enable",
            "--no-service",
            "--port",
            &port.to_string(),
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("collector: not running"));
    let out = run(&home, &user_home, &["doctor"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains(&format!("Nothing listens at http://127.0.0.1:{port}")),
        "{text}"
    );
    assert!(out.status.success(), "advisory findings never fail doctor");
}
