//! Harness version drift for `doctor` (strategy work item W28): the regression
//! eval is meant to run after every Claude Code or Cursor upgrade, so doctor
//! remembers the versions it has seen in `$SKILLCTL_HOME/harness-versions.json`
//! and reports, once, when one changed. Advisory like telemetry: a new version
//! is a reason to re-measure, not a problem to fix.
//!
//! Claude Code's version comes from `claude --version` (a plain CLI, run with
//! a timeout and no stdin); Cursor's from the event log, because Cursor passes
//! `cursor_version` to every hook and its `cursor` shell command is an
//! application launcher that must never be run from doctor.
use super::doctor::Problem;
use crate::eventlog::Event;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// How long `<binary> --version` may take before it is killed.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// File under the skillctl home that keeps every version sighting.
pub const STATE_FILE: &str = "harness-versions.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Sighting {
    pub version: String,
    /// The day (UTC, `YYYY-MM-DD`) this version was first seen.
    pub seen: String,
}

pub type History = BTreeMap<String, Vec<Sighting>>;

/// A harness doctor tracks: the key in the state file, the display name and
/// the fix suggested while its version is unknown.
pub const HARNESSES: &[(&str, &str, &str)] = &[
    (
        "claude_code",
        "Claude Code",
        "Put Claude Code's `claude` command on PATH, or ignore this if it is not used here.",
    ),
    (
        "cursor",
        "Cursor",
        "Cursor reports its version to hooks: register the skillctl-logger hook and use Cursor once, or ignore this if it is not used here.",
    ),
];

#[derive(Debug, Clone, Serialize)]
pub struct Harnesses {
    /// Versions detected on this run, by state-file key.
    pub detected: BTreeMap<String, Option<String>>,
    pub history: History,
    /// Advisory findings; `fixed` is always false and `path` always absent.
    pub findings: Vec<Problem>,
    /// Whether the history gained a sighting on this run.
    #[serde(skip)]
    pub changed: bool,
}

impl Harnesses {
    /// Render the findings in doctor's text layout, or nothing when there are none.
    pub fn text(&self) -> String {
        if self.findings.is_empty() {
            return String::new();
        }
        let mut lines =
            vec!["Harness versions (advisory; does not affect the exit status):".to_owned()];
        lines.extend(self.findings.iter().map(|item| {
            format!(
                "  [{}] {}\n      fix: {}",
                item.severity, item.message, item.fix
            )
        }));
        lines.join("\n")
    }
}

fn finding(check: &'static str, severity: &'static str, message: String, fix: String) -> Problem {
    Problem {
        check,
        severity,
        message,
        fix,
        path: None,
        fixed: false,
    }
}

/// Run `<binary> --version` with no stdin and a [`PROBE_TIMEOUT`], and keep
/// the first version-looking token of the first line (`2.1.291 (Claude Code)`
/// → `2.1.291`). `None` when the binary is missing, fails, hangs or prints no
/// version.
pub fn detect(binary: &str) -> Option<String> {
    let mut child = Command::new(binary)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < PROBE_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    if !status.success() {
        return None;
    }
    let mut text = String::new();
    io::Read::read_to_string(&mut child.stdout.take()?, &mut text).ok()?;
    version_in(&text)
}

/// The first token of the first line that looks like a version.
pub fn version_in(text: &str) -> Option<String> {
    text.lines()
        .next()?
        .split_whitespace()
        .find(|token| token.starts_with(|c: char| c.is_ascii_digit()) && token.contains('.'))
        .map(|token| token.trim_end_matches(',').to_owned())
}

/// The harness version of the most recent event from `tool`, if any event
/// carried one (Cursor's `cursor_version`, kept by the logger as
/// `harness_version`).
pub fn from_log(events: &[Event], tool: &str) -> Option<String> {
    events
        .iter()
        .filter(|event| event.tool == tool && event.harness_version.is_some())
        .max_by(|a, b| a.ts.cmp(&b.ts))
        .and_then(|event| event.harness_version.clone())
}

/// Compare the detected versions with the history and extend it. `today` is
/// the UTC day recorded for a new sighting.
pub fn inspect(
    detected: &[(&str, &str, Option<String>)],
    history: History,
    today: &str,
) -> Harnesses {
    let mut history = history;
    let mut findings = Vec::new();
    let mut changed = false;
    let mut detected_map = BTreeMap::new();
    for (key, label, version) in detected {
        detected_map.insert((*key).to_owned(), version.clone());
        let Some(version) = version else {
            findings.push(finding(
                "harness-missing",
                "info",
                format!("{label} version not detected, so its upgrades are not tracked"),
                HARNESSES
                    .iter()
                    .find(|(k, _, _)| k == key)
                    .map(|(_, _, fix)| (*fix).to_owned())
                    .unwrap_or_default(),
            ));
            continue;
        };
        let sightings = history.entry((*key).to_owned()).or_default();
        match sightings.last() {
            None => {
                findings.push(finding(
                    "harness-recorded",
                    "info",
                    format!("{label} {version} recorded; later runs report upgrades"),
                    "Nothing to do. Re-run the regression eval after each reported upgrade."
                        .to_owned(),
                ));
            }
            Some(last) if last.version != *version => {
                findings.push(finding(
                    "harness-upgraded",
                    "warning",
                    format!(
                        "{label} upgraded {} → {version} (previous version first seen {}); hook, activation and context behaviour may have changed",
                        last.version, last.seen
                    ),
                    "Re-run the regression eval and compare `skillctl log summary` windows before and after the upgrade.".to_owned(),
                ));
            }
            Some(_) => {}
        }
        if sightings.last().is_none_or(|last| last.version != *version) {
            sightings.push(Sighting {
                version: version.clone(),
                seen: today.to_owned(),
            });
            changed = true;
        }
    }
    Harnesses {
        detected: detected_map,
        history,
        findings,
        changed,
    }
}

/// The sightings recorded so far; a missing or unreadable file is empty.
pub fn load_history(path: &Path) -> History {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_history(path: &Path, history: &History) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(history).map_err(io::Error::other)?;
    std::fs::write(path, text + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checks(report: &Harnesses) -> Vec<&str> {
        report.findings.iter().map(|f| f.check).collect()
    }

    #[test]
    fn first_sighting_is_recorded_and_a_change_is_reported_once() {
        let detected = [
            ("claude_code", "Claude Code", Some("2.1.291".to_owned())),
            ("cursor", "Cursor", None),
        ];
        let first = inspect(&detected, History::new(), "2026-10-19");
        assert_eq!(checks(&first), ["harness-recorded", "harness-missing"]);
        assert!(first.changed);
        assert_eq!(first.history["claude_code"][0].version, "2.1.291");
        assert_eq!(first.detected["cursor"], None);

        let same = inspect(&detected, first.history.clone(), "2026-10-20");
        assert_eq!(checks(&same), ["harness-missing"]);
        assert!(!same.changed);

        let upgraded = [
            ("claude_code", "Claude Code", Some("2.1.300".to_owned())),
            ("cursor", "Cursor", Some("3.25.9".to_owned())),
        ];
        let report = inspect(&upgraded, same.history, "2026-10-21");
        assert_eq!(checks(&report), ["harness-upgraded", "harness-recorded"]);
        assert_eq!(report.findings[0].severity, "warning");
        assert!(
            report.findings[0]
                .message
                .contains("2.1.291 → 2.1.300 (previous version first seen 2026-10-19)")
        );
        assert_eq!(report.history["claude_code"].len(), 2);
        assert_eq!(report.history["cursor"][0].seen, "2026-10-21");
        assert!(report.changed);
        assert!(report.text().starts_with("Harness versions (advisory"));
    }

    #[test]
    fn versions_are_parsed_from_cli_output_and_the_log() {
        assert_eq!(
            version_in("2.1.291 (Claude Code)\n").as_deref(),
            Some("2.1.291")
        );
        assert_eq!(version_in("Cursor 3.25.9\nmore").as_deref(), Some("3.25.9"));
        assert_eq!(version_in("no version here"), None);
        assert_eq!(version_in(""), None);
        let events: Vec<Event> = crate::eventlog::parse(concat!(
            r#"{"ts":"2026-10-02T00:00:00Z","tool":"cursor","event":"Stop","harness_version":"3.24.0"}"#,
            "\n",
            r#"{"ts":"2026-10-05T00:00:00Z","tool":"cursor","event":"Stop","harness_version":"3.25.9"}"#,
            "\n",
            r#"{"ts":"2026-10-09T00:00:00Z","tool":"cursor","event":"Stop"}"#,
            "\n",
            r#"{"ts":"2026-10-10T00:00:00Z","tool":"claude-code","event":"Stop"}"#,
            "\n",
        ));
        assert_eq!(from_log(&events, "cursor").as_deref(), Some("3.25.9"));
        assert_eq!(from_log(&events, "claude-code"), None);
        assert_eq!(detect("/nonexistent/binary"), None);
    }

    #[test]
    fn history_round_trips_through_the_state_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("nested").join(STATE_FILE);
        assert!(load_history(&path).is_empty());
        let mut history = History::new();
        history.insert(
            "cursor".into(),
            vec![Sighting {
                version: "3.25.9".into(),
                seen: "2026-10-20".into(),
            }],
        );
        save_history(&path, &history).unwrap();
        assert_eq!(load_history(&path), history);
        std::fs::write(&path, "garbage").unwrap();
        assert!(load_history(&path).is_empty());
    }
}
