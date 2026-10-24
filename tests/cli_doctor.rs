use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

struct Env {
    _temp: tempfile::TempDir,
    home: std::path::PathBuf,
    user_home: std::path::PathBuf,
    root: std::path::PathBuf,
}

fn env() -> Env {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let user_home = temp.path().join("userhome");
    let root = temp.path().join("repo");
    fs::create_dir_all(&user_home).unwrap();
    fs::create_dir(&root).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .arg(&root)
            .status()
            .unwrap()
            .success()
    );
    Env {
        _temp: temp,
        home,
        user_home,
        root,
    }
}

fn run(env: &Env, dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", &env.home)
        .env("HOME", &env.user_home)
        .current_dir(dir)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}
fn ok(env: &Env, dir: &Path, args: &[&str]) -> Output {
    let out = run(env, dir, args);
    assert!(
        out.status.success(),
        "{args:?}: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}
fn checks(env: &Env, dir: &Path, args: &[&str]) -> (Option<i32>, Vec<String>, Vec<String>) {
    let mut full = vec!["--json"];
    full.extend_from_slice(args);
    let out = run(env, dir, &full);
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let problems = report["problems"].as_array().unwrap();
    let all = problems
        .iter()
        .map(|p| p["check"].as_str().unwrap().to_owned())
        .collect();
    let fixed = problems
        .iter()
        .filter(|p| p["fixed"] == true)
        .map(|p| p["check"].as_str().unwrap().to_owned())
        .collect();
    (out.status.code(), all, fixed)
}

#[test]
fn doctor_reports_and_fixes_link_drift_missing_instances_and_store_permissions() {
    let e = env();
    ok(&e, &e.root, &["new", "demo"]);
    ok(&e, &e.root, &["seal", "demo"]);
    ok(&e, &e.root, &["attach", "--new", "web", "--skills", "demo"]);
    let out = ok(&e, &e.root, &["doctor"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("No problems found."));

    // A deleted link is reported, exits 1, and --fix restores it.
    let link = e.root.join(".claude/skills/demo");
    fs::remove_file(&link).unwrap();
    let (code, all, _) = checks(&e, &e.root, &["doctor"]);
    assert_eq!(code, Some(1));
    assert_eq!(all, ["link-missing"]);
    let out = run(&e, &e.root, &["--json", "doctor"]);
    let error: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(error["error"], "doctor");
    let (code, _, fixed) = checks(&e, &e.root, &["doctor", "--fix"]);
    assert_eq!(code, Some(0));
    assert_eq!(fixed, ["link-missing"]);
    assert!(link.is_symlink());

    // A vanished instance folder is removed from the registry by --fix.
    let second = e.root.parent().unwrap().join("second");
    fs::create_dir(&second).unwrap();
    ok(&e, &second, &["attach", "--project", "web"]);
    fs::remove_dir_all(&second).unwrap();
    let (code, all, _) = checks(&e, &e.root, &["doctor"]);
    assert_eq!(code, Some(1));
    assert_eq!(all, ["instance-missing"]);
    let (code, _, fixed) = checks(&e, &e.root, &["doctor", "--fix"]);
    assert_eq!(code, Some(0));
    assert_eq!(fixed, ["instance-missing"]);
    let registry =
        skillctl::registry::Registry::from_yaml(&fs::read(e.home.join("registry.yml")).unwrap())
            .unwrap();
    assert_eq!(registry.projects["web"].instances.len(), 1);

    // Writable store entries are fixed; tampered content is only reported.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let stored = e.home.join("store/demo/0.1.0/demo/SKILL.md");
        fs::set_permissions(&stored, fs::Permissions::from_mode(0o644)).unwrap();
        let (code, all, _) = checks(&e, &e.root, &["doctor"]);
        assert_eq!(code, Some(1));
        assert_eq!(all, ["store-writable"]);
        let (code, _, fixed) = checks(&e, &e.root, &["doctor", "--fix"]);
        assert_eq!(code, Some(0));
        assert_eq!(fixed, ["store-writable"]);
        assert_eq!(
            fs::metadata(&stored).unwrap().permissions().mode() & 0o777,
            0o444
        );
        fs::set_permissions(&stored, fs::Permissions::from_mode(0o644)).unwrap();
        let mut text = fs::read_to_string(&stored).unwrap();
        text.push_str("\ntampered\n");
        fs::write(&stored, text).unwrap();
        let (code, all, fixed) = checks(&e, &e.root, &["doctor", "--fix"]);
        assert_eq!(code, Some(1));
        assert!(all.contains(&"hash-mismatch".to_owned()), "{all:?}");
        assert_eq!(fixed, ["store-writable"]);
    }
}

#[test]
fn doctor_reports_telemetry_without_changing_the_exit_status() {
    let e = env();
    ok(&e, &e.root, &["init"]);
    // A PATH holding only git and a scratch bin folder, so ccusage is absent for
    // sure, whatever the developer machine has installed.
    let git_dir = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .find(|dir| dir.join("git").is_file())
        .expect("git on PATH");
    let bin = e.root.parent().unwrap().join("bin");
    fs::create_dir(&bin).unwrap();
    let path = std::env::join_paths([bin.clone(), git_dir]).unwrap();
    let telemetry_vars = [
        "CLAUDE_CODE_ENABLE_TELEMETRY",
        "OTEL_METRICS_EXPORTER",
        "OTEL_LOGS_EXPORTER",
        "OTEL_EXPORTER_OTLP_PROTOCOL",
        "OTEL_EXPORTER_OTLP_ENDPOINT",
        "OTEL_LOG_TOOL_DETAILS",
    ];
    let doctor = |vars: &[(&str, &str)]| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_skillctl"));
        command
            .env("SKILLCTL_HOME", &e.home)
            .env("HOME", &e.user_home)
            .env("PATH", &path)
            .current_dir(&e.root)
            .args(["--json", "doctor"])
            .stdin(std::process::Stdio::null());
        for name in telemetry_vars {
            command.env_remove(name);
        }
        for (name, value) in vars {
            command.env(name, value);
        }
        let out = command.output().unwrap();
        let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        (out.status.code(), report)
    };

    // Nothing configured: advisory findings, but doctor still passes.
    let (code, report) = doctor(&[]);
    assert_eq!(code, Some(0));
    assert_eq!(report["problems"].as_array().unwrap().len(), 0);
    assert_eq!(report["telemetry"]["claude_code"]["enabled"], false);
    assert_eq!(report["telemetry"]["ccusage_on_path"], false);
    let checks: Vec<&str> = report["telemetry"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["check"].as_str().unwrap())
        .collect();
    assert_eq!(
        checks,
        [
            "telemetry-off",
            "ccusage-missing",
            "model-defaults",
            "cursor-telemetry"
        ]
    );
    let out = Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", &e.home)
        .env("HOME", &e.user_home)
        .env("PATH", &path)
        .env_remove("CLAUDE_CODE_ENABLE_TELEMETRY")
        .current_dir(&e.root)
        .arg("doctor")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Telemetry (advisory"), "{text}");
    assert!(
        text.contains("[warning] Claude Code OpenTelemetry export is off"),
        "{text}"
    );
    assert!(text.ends_with("No problems found.\n"), "{text}");

    // Everything configured and ccusage installed: only the Cursor note remains.
    // A remote endpoint is not probed; a local one is (tests/cli_telemetry.rs).
    fs::write(bin.join("ccusage"), "#!/bin/sh\n").unwrap();
    let (code, report) = doctor(&[
        ("CLAUDE_CODE_ENABLE_TELEMETRY", "1"),
        ("OTEL_METRICS_EXPORTER", "otlp"),
        ("OTEL_LOGS_EXPORTER", "otlp"),
        ("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc"),
        (
            "OTEL_EXPORTER_OTLP_ENDPOINT",
            "https://otel.example.com:4317",
        ),
        ("OTEL_LOG_TOOL_DETAILS", "1"),
    ]);
    assert_eq!(code, Some(0));
    assert_eq!(report["telemetry"]["claude_code"]["enabled"], true);
    assert_eq!(report["telemetry"]["ccusage_on_path"], true);
    let findings = report["telemetry"]["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0]["check"], "model-defaults");
    assert_eq!(findings[1]["check"], "cursor-telemetry");
    assert_eq!(findings[1]["severity"], "info");
}

#[test]
fn doctor_reports_missing_pins_and_shadowing_personal_copies() {
    let e = env();
    ok(&e, &e.root, &["new", "demo"]);
    ok(&e, &e.root, &["seal", "demo"]);
    ok(&e, &e.root, &["attach", "--new", "web", "--skills", "demo"]);
    // A personal copy in the user-level folder shadows the project skill.
    fs::create_dir_all(e.user_home.join(".claude/skills/demo")).unwrap();
    let (code, all, _) = checks(&e, &e.root, &["doctor"]);
    assert_eq!(code, Some(1));
    assert_eq!(all, ["shadowed"]);
    fs::remove_dir_all(e.user_home.join(".claude/skills/demo")).unwrap();
    // A pin whose version is not installed.
    let file = e.home.join("registry.yml");
    let text = fs::read_to_string(&file)
        .unwrap()
        .replace("demo: 0.1.0", "demo: 9.9.9");
    fs::write(&file, text).unwrap();
    let (code, all, _) = checks(&e, &e.root, &["doctor"]);
    assert_eq!(code, Some(1));
    assert!(all.contains(&"pin-missing".to_owned()), "{all:?}");
    assert!(all.contains(&"link-wrong".to_owned()), "{all:?}");
    // Dry runs report without touching anything.
    let before = fs::read(&file).unwrap();
    let (_, _, fixed) = checks(&e, &e.root, &["--dry-run", "doctor", "--fix"]);
    assert!(!fixed.is_empty());
    assert_eq!(fs::read(&file).unwrap(), before);
}

#[test]
fn doctor_audits_mcp_servers_from_both_harnesses_against_the_log() {
    let e = env();
    ok(&e, &e.root, &["init"]);
    let key = fs::canonicalize(&e.root)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    fs::write(
        e.user_home.join(".claude.json"),
        serde_json::json!({
            "mcpServers": {"github": {"command": "github-mcp-server"}},
            "projects": {key: {"disabledMcpjsonServers": ["jira"]}}
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        e.root.join(".mcp.json"),
        r#"{"mcpServers": {"jira": {"command": "jira-mcp"}, "docs": {"url": "http://localhost:9/mcp"}}}"#,
    )
    .unwrap();
    fs::create_dir_all(e.user_home.join(".cursor")).unwrap();
    fs::write(
        e.user_home.join(".cursor/mcp.json"),
        r#"{"mcpServers": {"docs": {"url": "http://localhost:9/mcp"}}}"#,
    )
    .unwrap();
    // Eight days of log in which only docs was ever called.
    fs::create_dir_all(e.home.join("log")).unwrap();
    fs::write(
        e.home.join("log/events.jsonl"),
        concat!(
            r#"{"ts":"2026-10-01T09:00:00Z","tool":"claude-code","event":"SessionStart"}"#, "\n",
            r#"{"ts":"2026-10-09T09:00:00Z","tool":"claude-code","event":"PostToolUse","tool_name":"mcp__docs__search","server":"docs"}"#, "\n",
        ),
    )
    .unwrap();
    let out = run(&e, &e.root, &["--json", "doctor"]);
    assert_eq!(out.status.code(), Some(0), "advisory only");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let servers: Vec<(&str, &str, bool, u64)> = report["mcp"]["servers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["name"].as_str().unwrap(),
                s["scope"].as_str().unwrap(),
                s["disabled"].as_bool().unwrap(),
                s["uses"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        servers,
        [
            ("github", "claude-user", false, 0),
            ("docs", "claude-mcpjson", false, 1),
            ("jira", "claude-mcpjson", true, 0),
            ("docs", "cursor-user", false, 1),
        ]
    );
    let findings: Vec<(&str, &str)> = report["mcp"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["check"].as_str().unwrap(),
                p["severity"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(findings, [("mcp-unused", "warning")]);
    assert!(
        report["mcp"]["findings"][0]["fix"]
            .as_str()
            .unwrap()
            .contains("gh CLI")
    );
    let out = run(&e, &e.root, &["doctor"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("MCP servers (advisory"), "{text}");
    assert!(
        text.contains("github [claude-user]: github-mcp-server; no tool call in the log"),
        "{text}"
    );
    assert!(text.contains("jira [claude-mcpjson] disabled"), "{text}");
}

#[cfg(unix)]
#[test]
fn doctor_records_harness_versions_and_reports_an_upgrade_once() {
    use std::os::unix::fs::PermissionsExt;
    let e = env();
    let bin = e.root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let fake = |version: &str| {
        fs::write(
            bin.join("claude"),
            format!("#!/bin/sh\necho \"{version} (Claude Code)\"\n"),
        )
        .unwrap();
        fs::set_permissions(bin.join("claude"), fs::Permissions::from_mode(0o755)).unwrap();
    };
    let doctor = |e: &Env| {
        let path = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let out = Command::new(env!("CARGO_BIN_EXE_skillctl"))
            .env("SKILLCTL_HOME", &e.home)
            .env("HOME", &e.user_home)
            .env("PATH", path)
            .current_dir(&e.root)
            .args(["--json", "doctor"])
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let checks: Vec<String> = report["harnesses"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["check"].as_str().unwrap().to_owned())
            .collect();
        (out.status.code(), report, checks)
    };
    ok(&e, &e.root, &["init"]);
    fs::create_dir_all(e.home.join("log")).unwrap();
    fs::write(
        e.home.join("log/events.jsonl"),
        r#"{"ts":"2026-10-19T10:00:00Z","tool":"cursor","event":"Stop","harness_version":"3.25.9"}"#,
    )
    .unwrap();
    fake("1.0.0");
    let (code, report, checks) = doctor(&e);
    assert_eq!(code, Some(0));
    assert_eq!(report["harnesses"]["detected"]["claude_code"], "1.0.0");
    assert_eq!(report["harnesses"]["detected"]["cursor"], "3.25.9");
    assert!(
        checks.contains(&"harness-recorded".to_owned()),
        "{checks:?}"
    );
    assert!(e.home.join("harness-versions.json").is_file());
    let (_, _, checks) = doctor(&e);
    assert!(
        !checks.contains(&"harness-recorded".to_owned()),
        "{checks:?}"
    );
    assert!(
        !checks.contains(&"harness-upgraded".to_owned()),
        "{checks:?}"
    );
    fake("1.1.0");
    let (code, report, checks) = doctor(&e);
    assert_eq!(code, Some(0), "advisory: never changes the exit status");
    assert!(
        checks.contains(&"harness-upgraded".to_owned()),
        "{checks:?}"
    );
    let upgraded = report["harnesses"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["check"] == "harness-upgraded")
        .unwrap();
    assert_eq!(upgraded["severity"], "warning");
    assert!(
        upgraded["message"]
            .as_str()
            .unwrap()
            .contains("Claude Code upgraded 1.0.0 → 1.1.0")
    );
    assert_eq!(
        report["harnesses"]["history"]["claude_code"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let (_, _, checks) = doctor(&e);
    assert!(
        !checks.contains(&"harness-upgraded".to_owned()),
        "reported once"
    );
}
