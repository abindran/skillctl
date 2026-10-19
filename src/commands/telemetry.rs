//! Observability baseline for `doctor`: does this machine export the token and
//! cost data that every later optimisation is measured against?
//!
//! Claude Code exports metrics and events over OpenTelemetry when a handful of
//! environment variables are set; `ccusage` sums its local session logs. Cursor
//! exports nothing, so its numbers come from the usage dashboard and from hook
//! logs. Findings here are advisory: they are reported next to doctor's problems
//! but never change its exit status, because a machine without telemetry is
//! unmeasured, not broken.
use super::doctor::Problem;
use serde::Serialize;

/// The export line suggested whenever Claude Code telemetry is off or incomplete.
/// `OTEL_LOG_TOOL_DETAILS` is part of it because without it `agent.name` is
/// exported as `custom`, which hides per-agent cost.
const SUGGESTED_EXPORTS: &str = "export CLAUDE_CODE_ENABLE_TELEMETRY=1 OTEL_METRICS_EXPORTER=otlp OTEL_LOGS_EXPORTER=otlp OTEL_EXPORTER_OTLP_PROTOCOL=grpc OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 OTEL_LOG_TOOL_DETAILS=1";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ClaudeCode {
    /// `CLAUDE_CODE_ENABLE_TELEMETRY=1`.
    pub enabled: bool,
    pub metrics_exporter: Option<String>,
    pub logs_exporter: Option<String>,
    pub otlp_protocol: Option<String>,
    pub otlp_endpoint: Option<String>,
    /// `OTEL_LOG_TOOL_DETAILS=1`, which unredacts `agent.name`.
    pub tool_details: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Telemetry {
    pub claude_code: ClaudeCode,
    pub ccusage_on_path: bool,
    /// Advisory findings; `fixed` is always false and `path` always absent.
    pub findings: Vec<Problem>,
}

impl Telemetry {
    /// Render the findings in doctor's text layout, or nothing when there are none.
    pub fn text(&self) -> String {
        if self.findings.is_empty() {
            return String::new();
        }
        let mut lines = vec!["Telemetry (advisory; does not affect the exit status):".to_owned()];
        lines.extend(self.findings.iter().map(|item| {
            format!(
                "  [{}] {}\n      fix: {}",
                item.severity, item.message, item.fix
            )
        }));
        lines.join("\n")
    }
}

/// What the event log says about Cursor: how many events it has written, when
/// the last one was, and whether any hook is registered at all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CursorEvidence {
    pub hooks_registered: usize,
    pub events: usize,
    pub last_event: Option<String>,
}

/// Model and effort defaults read from `~/.claude/settings.json`: the main
/// `model`, and `CLAUDE_CODE_SUBAGENT_MODEL` from its `env` block (the
/// process environment also counts).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ModelDefaults {
    pub main_model: Option<String>,
    pub subagent_model: Option<String>,
}

impl ModelDefaults {
    pub fn from_settings(
        settings: &serde_json::Value,
        var: impl Fn(&str) -> Option<String>,
    ) -> Self {
        let env = settings.get("env");
        let from_env = |key: &str| {
            var(key).or_else(|| {
                env.and_then(|env| env.get(key))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
        };
        Self {
            main_model: settings
                .get("model")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            subagent_model: from_env("CLAUDE_CODE_SUBAGENT_MODEL"),
        }
    }
}

/// Inspect the environment through injected lookups so tests need no process state.
pub fn inspect(var: impl Fn(&str) -> Option<String>, on_path: impl Fn(&str) -> bool) -> Telemetry {
    inspect_with_cursor(var, on_path, &CursorEvidence::default())
}

/// `inspect`, plus what the log shows about Cursor running the registered hooks.
pub fn inspect_with_cursor(
    var: impl Fn(&str) -> Option<String>,
    on_path: impl Fn(&str) -> bool,
    cursor: &CursorEvidence,
) -> Telemetry {
    inspect_full(var, on_path, cursor, &ModelDefaults::default())
}

/// `inspect_with_cursor`, plus the model and effort defaults (W25).
pub fn inspect_full(
    var: impl Fn(&str) -> Option<String>,
    on_path: impl Fn(&str) -> bool,
    cursor: &CursorEvidence,
    models: &ModelDefaults,
) -> Telemetry {
    let set = |name: &str| {
        var(name)
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    };
    // An exporter of `none` is the same as no exporter.
    let exporter = |name: &str| set(name).filter(|value| value != "none");
    let claude_code = ClaudeCode {
        enabled: set("CLAUDE_CODE_ENABLE_TELEMETRY").as_deref() == Some("1"),
        metrics_exporter: exporter("OTEL_METRICS_EXPORTER"),
        logs_exporter: exporter("OTEL_LOGS_EXPORTER"),
        otlp_protocol: set("OTEL_EXPORTER_OTLP_PROTOCOL"),
        otlp_endpoint: set("OTEL_EXPORTER_OTLP_ENDPOINT"),
        tool_details: set("OTEL_LOG_TOOL_DETAILS").as_deref() == Some("1"),
    };
    let mut findings = Vec::new();
    let finding =
        |check: &'static str, severity: &'static str, message: String, fix: &str| Problem {
            check,
            severity,
            message,
            fix: fix.to_owned(),
            path: None,
            fixed: false,
        };

    if !claude_code.enabled {
        findings.push(finding(
            "telemetry-off",
            "warning",
            "Claude Code OpenTelemetry export is off (CLAUDE_CODE_ENABLE_TELEMETRY is not 1), so tokens and cost per skill and agent are not recorded".to_owned(),
            SUGGESTED_EXPORTS,
        ));
    } else {
        match (&claude_code.metrics_exporter, &claude_code.logs_exporter) {
            (None, None) => findings.push(finding(
                "telemetry-no-exporter",
                "warning",
                "Claude Code telemetry is enabled but neither OTEL_METRICS_EXPORTER nor OTEL_LOGS_EXPORTER is set, so nothing is exported".to_owned(),
                SUGGESTED_EXPORTS,
            )),
            (None, Some(_)) => findings.push(finding(
                "telemetry-no-metrics",
                "warning",
                "OTEL_METRICS_EXPORTER is not set; claude_code.token.usage and claude_code.cost.usage are metrics, so totals by skill.name and agent.name are not exported".to_owned(),
                "export OTEL_METRICS_EXPORTER=otlp",
            )),
            _ => {}
        }
        // Each signal that goes over OTLP needs a protocol and an endpoint, either
        // generic or per signal.
        let otlp_signals: Vec<&str> = [
            ("METRICS", &claude_code.metrics_exporter),
            ("LOGS", &claude_code.logs_exporter),
        ]
        .into_iter()
        .filter(|(_, exporter)| {
            exporter
                .as_deref()
                .is_some_and(|value| value.split(',').any(|part| part.trim() == "otlp"))
        })
        .map(|(signal, _)| signal)
        .collect();
        for (suffix, generic, check, example) in [
            (
                "PROTOCOL",
                &claude_code.otlp_protocol,
                "telemetry-otlp-protocol",
                "grpc",
            ),
            (
                "ENDPOINT",
                &claude_code.otlp_endpoint,
                "telemetry-otlp-endpoint",
                "http://localhost:4317",
            ),
        ] {
            let missing: Vec<&str> = otlp_signals
                .iter()
                .copied()
                .filter(|signal| {
                    generic.is_none()
                        && set(&format!("OTEL_EXPORTER_OTLP_{signal}_{suffix}")).is_none()
                })
                .collect();
            if !missing.is_empty() {
                findings.push(finding(
                    check,
                    "warning",
                    format!(
                        "OTLP export of {} has no OTEL_EXPORTER_OTLP_{suffix}, which Claude Code requires",
                        missing.join(" and ").to_lowercase()
                    ),
                    &format!("export OTEL_EXPORTER_OTLP_{suffix}={example}"),
                ));
            }
        }
        if !claude_code.tool_details {
            findings.push(finding(
                "telemetry-agent-redacted",
                "warning",
                "OTEL_LOG_TOOL_DETAILS is not 1, so agent.name is exported as \"custom\" and per-agent cost cannot be told apart".to_owned(),
                "export OTEL_LOG_TOOL_DETAILS=1",
            ));
        }
    }

    let ccusage_on_path = on_path("ccusage");
    if !ccusage_on_path {
        findings.push(finding(
            "ccusage-missing",
            "warning",
            "ccusage is not on PATH; it sums tokens and cost from Claude Code's local session logs"
                .to_owned(),
            "npm install -g ccusage (or run it ad hoc with npx ccusage)",
        ));
    }

    // Model routing: the strategy's default is Sonnet for the main agent and
    // Haiku for read-heavy subagents, Opus only by escalation per task.
    match &models.subagent_model {
        None => findings.push(finding(
            "model-defaults",
            "info",
            format!(
                "Main model: {}; subagents inherit it (CLAUDE_CODE_SUBAGENT_MODEL is unset), so read-only exploration runs at full price",
                models.main_model.as_deref().unwrap_or("harness default")
            ),
            "Add \"env\": {\"CLAUDE_CODE_SUBAGENT_MODEL\": \"haiku\"} to ~/.claude/settings.json; agents that set their own model (explore, reviewer) are unaffected",
        )),
        Some(subagent) => findings.push(finding(
            "model-defaults",
            "info",
            format!(
                "Main model: {}; subagents default to {subagent}",
                models.main_model.as_deref().unwrap_or("harness default")
            ),
            "Nothing to set; switch the main model per task with /model and the reasoning budget with /effort",
        )),
    }

    // Cursor has no telemetry export; the hook log is its instrument, and the
    // log also shows whether Cursor imports the registered hooks at all.
    match (cursor.hooks_registered, cursor.events, &cursor.last_event) {
        (0, _, _) => findings.push(finding(
            "cursor-telemetry",
            "info",
            "Cursor exports no telemetry; its baseline comes from the usage dashboard (cursor.com, Settings, Usage) and from hook logs".to_owned(),
            "Nothing to set; record Cursor numbers from the dashboard and the hook log",
        )),
        (hooks, 0, _) => findings.push(finding(
            "cursor-imports",
            "warning",
            format!(
                "No event from Cursor in the log although {hooks} hook(s) are registered; Cursor runs Claude Code hooks only when Third-Party Imports is on"
            ),
            "In Cursor open Settings, Agents, Third-Party Imports and enable \"Include Third-Party Plugins, Skills, and Other Configs\" (on by default), then send one prompt in Cursor and run skillctl doctor again",
        )),
        (_, events, last) => findings.push(finding(
            "cursor-imports",
            "info",
            format!(
                "Cursor runs the registered hooks ({events} event(s), last {}); Third-Party Imports is on",
                last.as_deref().unwrap_or("unknown")
            ),
            "Nothing to do; Cursor's cost numbers still come from its usage dashboard",
        )),
    }

    Telemetry {
        claude_code,
        ccusage_on_path,
        findings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn inspect_with(vars: &[(&str, &str)], ccusage: bool) -> Telemetry {
        let vars: BTreeMap<&str, &str> = vars.iter().copied().collect();
        inspect(
            |name| vars.get(name).map(|value| (*value).to_owned()),
            |binary| ccusage && binary == "ccusage",
        )
    }

    fn checks(telemetry: &Telemetry) -> Vec<&str> {
        telemetry.findings.iter().map(|item| item.check).collect()
    }

    #[test]
    fn unset_environment_reports_export_off_and_missing_ccusage() {
        let telemetry = inspect_with(&[], false);
        assert!(!telemetry.claude_code.enabled);
        assert!(!telemetry.ccusage_on_path);
        assert_eq!(
            checks(&telemetry),
            [
                "telemetry-off",
                "ccusage-missing",
                "model-defaults",
                "cursor-telemetry"
            ]
        );
        assert!(
            telemetry.findings[0]
                .fix
                .contains("CLAUDE_CODE_ENABLE_TELEMETRY=1")
        );
        assert_eq!(telemetry.findings[2].severity, "info");
        assert!(telemetry.text().starts_with("Telemetry (advisory"));
    }

    #[test]
    fn complete_configuration_leaves_only_the_cursor_note() {
        let telemetry = inspect_with(
            &[
                ("CLAUDE_CODE_ENABLE_TELEMETRY", "1"),
                ("OTEL_METRICS_EXPORTER", "otlp"),
                ("OTEL_LOGS_EXPORTER", "otlp"),
                ("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc"),
                ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://localhost:4317"),
                ("OTEL_LOG_TOOL_DETAILS", "1"),
            ],
            true,
        );
        assert!(telemetry.claude_code.enabled);
        assert!(telemetry.claude_code.tool_details);
        assert!(telemetry.ccusage_on_path);
        assert_eq!(checks(&telemetry), ["model-defaults", "cursor-telemetry"]);
    }

    #[test]
    fn enabled_without_exporters_or_tool_details_is_reported() {
        let telemetry = inspect_with(&[("CLAUDE_CODE_ENABLE_TELEMETRY", "1")], true);
        assert_eq!(
            checks(&telemetry),
            [
                "telemetry-no-exporter",
                "telemetry-agent-redacted",
                "model-defaults",
                "cursor-telemetry"
            ]
        );
        let telemetry = inspect_with(
            &[
                ("CLAUDE_CODE_ENABLE_TELEMETRY", "1"),
                ("OTEL_METRICS_EXPORTER", "none"),
                ("OTEL_LOGS_EXPORTER", "console"),
                ("OTEL_LOG_TOOL_DETAILS", "1"),
            ],
            true,
        );
        assert_eq!(
            checks(&telemetry),
            ["telemetry-no-metrics", "model-defaults", "cursor-telemetry"]
        );
    }

    #[test]
    fn otlp_signals_need_protocol_and_endpoint_generic_or_per_signal() {
        let telemetry = inspect_with(
            &[
                ("CLAUDE_CODE_ENABLE_TELEMETRY", "1"),
                ("OTEL_METRICS_EXPORTER", "console,otlp"),
                ("OTEL_LOGS_EXPORTER", "console"),
                ("OTEL_LOG_TOOL_DETAILS", "1"),
            ],
            true,
        );
        assert_eq!(
            checks(&telemetry),
            [
                "telemetry-otlp-protocol",
                "telemetry-otlp-endpoint",
                "model-defaults",
                "cursor-telemetry"
            ]
        );
        assert!(telemetry.findings[0].message.contains("metrics has no"));
        // Per-signal overrides satisfy the check for their signal only.
        let telemetry = inspect_with(
            &[
                ("CLAUDE_CODE_ENABLE_TELEMETRY", "1"),
                ("OTEL_METRICS_EXPORTER", "otlp"),
                ("OTEL_LOGS_EXPORTER", "otlp"),
                ("OTEL_EXPORTER_OTLP_METRICS_PROTOCOL", "http/protobuf"),
                (
                    "OTEL_EXPORTER_OTLP_METRICS_ENDPOINT",
                    "http://collector:4318",
                ),
                ("OTEL_EXPORTER_OTLP_LOGS_PROTOCOL", "grpc"),
                ("OTEL_LOG_TOOL_DETAILS", "1"),
            ],
            true,
        );
        assert_eq!(
            checks(&telemetry),
            [
                "telemetry-otlp-endpoint",
                "model-defaults",
                "cursor-telemetry"
            ]
        );
        assert!(
            telemetry.findings[0]
                .message
                .contains("logs has no OTEL_EXPORTER_OTLP_ENDPOINT")
        );
    }

    #[test]
    fn cursor_import_evidence_drives_the_cursor_finding() {
        let full = [
            ("CLAUDE_CODE_ENABLE_TELEMETRY", "1"),
            ("OTEL_METRICS_EXPORTER", "otlp"),
            ("OTEL_LOGS_EXPORTER", "otlp"),
            ("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc"),
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://localhost:4317"),
            ("OTEL_LOG_TOOL_DETAILS", "1"),
        ];
        let vars: BTreeMap<&str, &str> = full.iter().copied().collect();
        let run = |cursor: CursorEvidence| {
            inspect_with_cursor(
                |name| vars.get(name).map(|v| (*v).to_owned()),
                |_| true,
                &cursor,
            )
        };
        let none = run(CursorEvidence {
            hooks_registered: 1,
            events: 0,
            last_event: None,
        });
        let cursor_of = |t: &Telemetry| {
            t.findings
                .iter()
                .find(|f| f.check.starts_with("cursor-"))
                .cloned()
                .unwrap()
        };
        assert_eq!(cursor_of(&none).check, "cursor-imports");
        assert_eq!(cursor_of(&none).severity, "warning");
        let seen = run(CursorEvidence {
            hooks_registered: 1,
            events: 12,
            last_event: Some("2026-10-19T12:00:00Z".into()),
        });
        assert_eq!(cursor_of(&seen).check, "cursor-imports");
        assert_eq!(cursor_of(&seen).severity, "info");
        assert!(
            cursor_of(&seen)
                .message
                .contains("12 event(s), last 2026-10-19T12:00:00Z")
        );
        let no_hooks = run(CursorEvidence::default());
        assert_eq!(cursor_of(&no_hooks).check, "cursor-telemetry");
    }

    #[test]
    fn model_defaults_come_from_settings_or_the_environment() {
        let settings =
            serde_json::json!({"model": "sonnet", "env": {"CLAUDE_CODE_SUBAGENT_MODEL": "haiku"}});
        let defaults = ModelDefaults::from_settings(&settings, |_| None);
        assert_eq!(defaults.main_model.as_deref(), Some("sonnet"));
        assert_eq!(defaults.subagent_model.as_deref(), Some("haiku"));
        let overridden = ModelDefaults::from_settings(&settings, |key| {
            (key == "CLAUDE_CODE_SUBAGENT_MODEL").then(|| "opus".to_owned())
        });
        assert_eq!(overridden.subagent_model.as_deref(), Some("opus"));
        let none = ModelDefaults::from_settings(&serde_json::json!({}), |_| None);
        assert_eq!(none, ModelDefaults::default());
        let telemetry = inspect_full(|_| None, |_| true, &CursorEvidence::default(), &none);
        let item = telemetry
            .findings
            .iter()
            .find(|f| f.check == "model-defaults")
            .unwrap();
        assert!(item.message.contains("subagents inherit it"));
        let telemetry = inspect_full(|_| None, |_| true, &CursorEvidence::default(), &defaults);
        let item = telemetry
            .findings
            .iter()
            .find(|f| f.check == "model-defaults")
            .unwrap();
        assert!(item.message.contains("subagents default to haiku"));
    }

    #[test]
    fn blank_values_count_as_unset() {
        let telemetry = inspect_with(
            &[
                ("CLAUDE_CODE_ENABLE_TELEMETRY", " "),
                ("OTEL_METRICS_EXPORTER", ""),
            ],
            false,
        );
        assert!(!telemetry.claude_code.enabled);
        assert_eq!(telemetry.claude_code.metrics_exporter, None);
        assert_eq!(checks(&telemetry)[0], "telemetry-off");
    }
}
