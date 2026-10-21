//! MCP server audit for `doctor`: which servers each harness will connect to
//! from this folder, and which of them the event log has never seen used.
//! Every connected server's tool definitions are loaded into context on every
//! turn, so an unused server is pure cost. Advisory, like the telemetry check.
use super::doctor::Problem;
use crate::error::Error;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
};

/// Days of log history before "never used" is called a warning rather than
/// "not measured yet".
pub const UNUSED_AFTER_DAYS: i64 = 7;

#[derive(Debug, Clone, Serialize)]
pub struct Server {
    pub name: String,
    /// `claude-user`, `claude-project`, `claude-mcpjson`, `cursor-user`, `cursor-project`.
    pub scope: &'static str,
    pub source: PathBuf,
    /// The command or URL, for recognising what the server is.
    pub transport: Option<String>,
    pub disabled: bool,
    pub uses: usize,
    pub last_used: Option<String>,
}

/// A server the log saw only through a gateway (`call_tool` forwarding to
/// `<server>__<tool>`): it is configured inside the gateway, not in any file
/// doctor reads.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BehindGateway {
    pub name: String,
    pub gateway: String,
    pub uses: usize,
    pub last_used: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Audit {
    pub servers: Vec<Server>,
    /// Servers reached through a gateway, from the log (logger 1.2.4+).
    pub behind_gateways: Vec<BehindGateway>,
    pub log_events: usize,
    pub log_from: Option<String>,
    pub log_to: Option<String>,
    pub findings: Vec<Problem>,
}

impl Audit {
    pub fn text(&self) -> String {
        if self.servers.is_empty() && self.findings.is_empty() && self.behind_gateways.is_empty() {
            return String::new();
        }
        let mut lines = vec![format!(
            "MCP servers (advisory; every connected server's tool definitions are loaded on every turn):"
        )];
        for server in &self.servers {
            lines.push(format!(
                "  {} [{}]{}: {}{}",
                server.name,
                server.scope,
                if server.disabled { " disabled" } else { "" },
                server.transport.as_deref().unwrap_or("(no command or url)"),
                match (&server.last_used, server.uses) {
                    (Some(last), uses) => format!("; {uses} tool call(s), last {last}"),
                    (None, _) => "; no tool call in the log".to_owned(),
                }
            ));
        }
        for server in &self.behind_gateways {
            lines.push(format!(
                "  {} [via {}]: {} tool call(s), last {}",
                server.name, server.gateway, server.uses, server.last_used
            ));
        }
        for item in &self.findings {
            lines.push(format!(
                "  [{}] {}\n      fix: {}",
                item.severity, item.message, item.fix
            ));
        }
        lines.join("\n")
    }
}

fn read_json(path: &Path) -> Result<Option<Value>, Error> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).ok()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(Error::Io {
            operation: format!("Read {}", path.display()),
            source,
            hint: "Check the file and its read permissions.".into(),
        }),
    }
}

fn transport(config: &Value) -> Option<String> {
    if let Some(url) = config.get("url").and_then(Value::as_str) {
        return Some(url.to_owned());
    }
    let command = config.get("command").and_then(Value::as_str)?;
    // The first non-flag argument names the package for `npx -y <pkg>` style
    // launchers; flags alone say nothing about what the server is.
    let first_arg = config
        .get("args")
        .and_then(Value::as_array)
        .and_then(|args| {
            args.iter()
                .filter_map(Value::as_str)
                .find(|arg| !arg.starts_with('-'))
        });
    Some(match first_arg {
        Some(arg) => format!("{command} {arg}"),
        None => command.to_owned(),
    })
}

fn servers_in(
    value: &Value,
    scope: &'static str,
    source: &Path,
    disabled: &[String],
    out: &mut Vec<Server>,
) {
    let Some(map) = value.get("mcpServers").and_then(Value::as_object) else {
        return;
    };
    for (name, config) in map {
        out.push(Server {
            name: name.clone(),
            scope,
            source: source.to_path_buf(),
            transport: transport(config),
            disabled: disabled.iter().any(|item| item == name),
            uses: 0,
            last_used: None,
        });
    }
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// CLI alternatives the strategy recommends over an always-connected server.
fn cli_alternative(name: &str, transport: Option<&str>) -> Option<&'static str> {
    let haystack = format!("{name} {}", transport.unwrap_or("")).to_lowercase();
    [
        ("github", "the gh CLI from Bash"),
        ("gitlab", "the glab CLI from Bash"),
        ("aws", "the aws CLI from Bash"),
        ("filesystem", "the harness's own Read, Edit and Glob tools"),
        ("fetch", "WebFetch or curl from Bash"),
        ("git", "git from Bash"),
    ]
    .iter()
    .find(|(key, _)| haystack.contains(key))
    .map(|(_, alternative)| *alternative)
}

/// Audit the servers visible from `folder` for the user `user_home`, scoring
/// usage from the logger's JSONL at `log`.
pub fn audit(user_home: &Path, folder: Option<&Path>, log: &Path) -> Result<Audit, Error> {
    let mut servers = Vec::new();
    let claude_json = user_home.join(".claude.json");
    let mut disabled_mcpjson = Vec::new();
    if let Some(value) = read_json(&claude_json)? {
        servers_in(&value, "claude-user", &claude_json, &[], &mut servers);
        if let Some(folder) = folder {
            if let Some(project) = value
                .get("projects")
                .and_then(|projects| projects.get(folder.to_string_lossy().as_ref()))
            {
                servers_in(project, "claude-project", &claude_json, &[], &mut servers);
                disabled_mcpjson = strings(project.get("disabledMcpjsonServers"));
            }
        }
    }
    if let Some(folder) = folder {
        let mcp_json = folder.join(".mcp.json");
        if let Some(value) = read_json(&mcp_json)? {
            servers_in(
                &value,
                "claude-mcpjson",
                &mcp_json,
                &disabled_mcpjson,
                &mut servers,
            );
        }
        let cursor_project = folder.join(".cursor/mcp.json");
        if let Some(value) = read_json(&cursor_project)? {
            servers_in(&value, "cursor-project", &cursor_project, &[], &mut servers);
        }
    }
    let cursor_user = user_home.join(".cursor/mcp.json");
    if let Some(value) = read_json(&cursor_user)? {
        servers_in(&value, "cursor-user", &cursor_user, &[], &mut servers);
    }

    // Usage from the logger: PostToolUse events whose tool is mcp__<server>__<tool>.
    let mut uses: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut behind: BTreeMap<(String, String), (usize, String)> = BTreeMap::new();
    let mut log_events = 0;
    let mut log_from: Option<String> = None;
    let mut log_to: Option<String> = None;
    if let Ok(text) = std::fs::read_to_string(log) {
        for line in text.lines() {
            let Ok(event) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            log_events += 1;
            if let Some(ts) = event.get("ts").and_then(Value::as_str) {
                if log_from.as_deref().is_none_or(|from| ts < from) {
                    log_from = Some(ts.to_owned());
                }
                if log_to.as_deref().is_none_or(|to| ts > to) {
                    log_to = Some(ts.to_owned());
                }
                let server = event
                    .get("server")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| {
                        event
                            .get("tool_name")
                            .and_then(Value::as_str)
                            .and_then(|tool| tool.strip_prefix("mcp__"))
                            .and_then(|rest| rest.split("__").next())
                            .map(str::to_owned)
                    });
                let count = |entry: &mut (usize, String)| {
                    entry.0 += 1;
                    if ts > entry.1.as_str() {
                        entry.1 = ts.to_owned();
                    }
                };
                // A forwarded call uses the gateway too: it is what is loaded
                // every turn, so it must not look unused.
                let via = event.get("via").and_then(Value::as_str);
                if let (Some(server), Some(via)) = (&server, via) {
                    count(
                        behind
                            .entry((server.clone(), via.to_owned()))
                            .or_insert((0, ts.to_owned())),
                    );
                    count(uses.entry(via.to_owned()).or_insert((0, ts.to_owned())));
                } else if let Some(server) = server {
                    count(uses.entry(server).or_insert((0, ts.to_owned())));
                }
            }
        }
    }
    for server in &mut servers {
        if let Some((count, last)) = uses.get(&server.name) {
            server.uses = *count;
            server.last_used = Some(last.clone());
        }
    }
    let span_days = match (&log_from, &log_to) {
        (Some(from), Some(to)) => {
            let parse =
                |text: &str| DateTime::parse_from_rfc3339(text).map(|t| t.with_timezone(&Utc));
            match (parse(from), parse(to)) {
                (Ok(from), Ok(to)) => (to - from).num_days(),
                _ => 0,
            }
        }
        _ => 0,
    };

    let mut findings = Vec::new();
    for server in servers
        .iter()
        .filter(|server| !server.disabled && server.uses == 0)
    {
        let alternative = cli_alternative(&server.name, server.transport.as_deref());
        let fix = match alternative {
            Some(alternative) => format!(
                "Remove '{}' from {} and use {alternative} instead; re-add it if a task needs it",
                server.name,
                server.source.display()
            ),
            None => format!(
                "Remove '{}' from {} if nothing needs it, or disable it and re-enable per task",
                server.name,
                server.source.display()
            ),
        };
        if span_days >= UNUSED_AFTER_DAYS {
            findings.push(Problem {
                check: "mcp-unused",
                severity: "warning",
                message: format!(
                    "MCP server '{}' [{}] has no recorded tool call in {log_events} logged events over {span_days} days; its tool definitions still load every turn",
                    server.name, server.scope
                ),
                fix,
                path: Some(server.source.clone()),
                fixed: false,
            });
        } else {
            findings.push(Problem {
                check: "mcp-unmeasured",
                severity: "info",
                message: format!(
                    "MCP server '{}' [{}] has no recorded tool call yet; the log covers {span_days} day(s), so usage is not measured until {UNUSED_AFTER_DAYS}",
                    server.name, server.scope
                ),
                fix: match alternative {
                    Some(alternative) => format!("Keep logging; if it stays unused, {alternative} replaces it"),
                    None => "Keep logging; decide once a week of sessions is recorded".to_owned(),
                },
                path: Some(server.source.clone()),
                fixed: false,
            });
        }
    }
    let behind_gateways = behind
        .into_iter()
        .map(|((name, gateway), (uses, last_used))| BehindGateway {
            name,
            gateway,
            uses,
            last_used,
        })
        .collect();
    Ok(Audit {
        servers,
        behind_gateways,
        log_events,
        log_from,
        log_to,
        findings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn servers_are_collected_from_every_source_with_usage_and_findings() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let project = temp.path().join("proj");
        fs::create_dir_all(&project).unwrap();
        let project_key = project.to_string_lossy().into_owned();
        write(
            &home.join(".claude.json"),
            &serde_json::json!({
                "mcpServers": {"obsidian": {"type": "stdio", "command": "npx", "args": ["-y", "obsidian-mcp"], "env": {"TOKEN": "x"}}},
                "projects": {project_key: {
                    "mcpServers": {"next-devtools": {"url": "http://localhost:3000/mcp"}},
                    "disabledMcpjsonServers": ["github"]
                }}
            })
            .to_string(),
        );
        write(
            &project.join(".mcp.json"),
            r#"{"mcpServers": {"github": {"command": "gh-mcp"}, "jira": {"command": "jira-mcp"}}}"#,
        );
        write(
            &home.join(".cursor/mcp.json"),
            r#"{"mcpServers": {"obsidian": {"command": "npx", "args": ["obsidian-mcp"]}}}"#,
        );
        let log = temp.path().join("events.jsonl");
        write(
            &log,
            concat!(
                r#"{"ts":"2026-10-01T10:00:00Z","event":"SessionStart"}"#,
                "\n",
                r#"{"ts":"2026-10-05T10:00:00Z","event":"PostToolUse","tool_name":"mcp__obsidian__search"}"#,
                "\n",
                r#"{"ts":"2026-10-12T10:00:00Z","event":"PostToolUse","tool_name":"mcp__obsidian__read","server":"obsidian"}"#,
                "\n",
                "not json\n",
            ),
        );
        let report = audit(&home, Some(&project), &log).unwrap();
        let names: Vec<(&str, &str, bool, usize)> = report
            .servers
            .iter()
            .map(|s| (s.name.as_str(), s.scope, s.disabled, s.uses))
            .collect();
        assert_eq!(
            names,
            [
                ("obsidian", "claude-user", false, 2),
                ("next-devtools", "claude-project", false, 0),
                ("github", "claude-mcpjson", true, 0),
                ("jira", "claude-mcpjson", false, 0),
                ("obsidian", "cursor-user", false, 2),
            ]
        );
        assert_eq!(
            report.servers[0].transport.as_deref(),
            Some("npx obsidian-mcp")
        );
        assert_eq!(
            report.servers[0].last_used.as_deref(),
            Some("2026-10-12T10:00:00Z")
        );
        assert_eq!(report.log_events, 3);
        // 11 days of log: the unused, enabled servers are warnings; disabled is skipped.
        let checks: Vec<(&str, &str)> = report
            .findings
            .iter()
            .map(|p| (p.check, p.message.split('\'').nth(1).unwrap()))
            .collect();
        assert_eq!(
            checks,
            [("mcp-unused", "next-devtools"), ("mcp-unused", "jira")]
        );
        assert!(
            report
                .text()
                .contains("obsidian [claude-user]: npx obsidian-mcp; 2 tool call(s)")
        );
        // A young log downgrades to info and names a CLI alternative when known.
        write(
            &log,
            r#"{"ts":"2026-10-01T10:00:00Z","event":"SessionStart"}"#,
        );
        fs::remove_file(home.join(".claude.json")).unwrap();
        write(
            &home.join(".claude.json"),
            &serde_json::json!({"mcpServers": {"github": {"command": "github-mcp-server"}}})
                .to_string(),
        );
        let report = audit(&home, None, &log).unwrap();
        assert_eq!(report.findings.len(), 2);
        assert!(report.findings.iter().all(|p| p.check == "mcp-unmeasured"));
        assert!(report.findings[0].fix.contains("gh CLI"));
    }

    #[test]
    fn missing_files_and_unparsable_json_yield_an_empty_audit() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        write(&home.join(".claude.json"), "{ not json");
        let report = audit(&home, None, &temp.path().join("none.jsonl")).unwrap();
        assert!(report.servers.is_empty());
        assert!(report.findings.is_empty());
        assert_eq!(report.text(), "");
    }

    #[test]
    fn calls_through_a_gateway_count_for_it_and_list_the_server_behind() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join(".claude.json"),
            r#"{"mcpServers": {"mcpgw": {"command": "mcpgw", "args": ["connect"]}}}"#,
        )
        .unwrap();
        let log = temp.path().join("events.jsonl");
        std::fs::write(
            &log,
            concat!(
                r#"{"ts":"2026-10-21T10:00:00Z","event":"PostToolUse","tool_name":"mcp__mcpgw__call_tool","server":"zrepo","mcp_tool":"compare","via":"mcpgw"}"#,
                "\n",
                r#"{"ts":"2026-10-21T11:00:00Z","event":"PostToolUse","tool_name":"mcp__mcpgw__call_tool","server":"zrepo","mcp_tool":"get_mr","via":"mcpgw"}"#,
                "\n",
                r#"{"ts":"2026-10-21T12:00:00Z","event":"PostToolUse","tool_name":"mcp__mcpgw__search_tools","server":"mcpgw","mcp_tool":"search_tools"}"#,
                "\n",
            ),
        )
        .unwrap();
        let report = audit(&home, None, &log).unwrap();
        assert_eq!(report.servers[0].name, "mcpgw");
        assert_eq!(report.servers[0].uses, 3, "forwarded calls use the gateway");
        assert_eq!(
            report.behind_gateways,
            [BehindGateway {
                name: "zrepo".into(),
                gateway: "mcpgw".into(),
                uses: 2,
                last_used: "2026-10-21T11:00:00Z".into(),
            }]
        );
        assert!(report.findings.is_empty());
        assert!(
            report
                .text()
                .contains("zrepo [via mcpgw]: 2 tool call(s), last 2026-10-21T11:00:00Z")
        );
    }
}
