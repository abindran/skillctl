//! `log summary [--days N] [--until <time>] [--strict]`: what the event log
//! says about the last N days against the N days before, with the alerts of
//! strategy work item W29. Activation drops and compaction rises are read
//! from the log; cache-hit rate and cost are not in it and are pointed at
//! `ccusage` and the OpenTelemetry backend instead.
use crate::{
    cli::LogCommand,
    context::Context,
    error::Error,
    eventlog::{self, Event},
};
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Prompts each window needs before an activation drop is reported.
pub const MIN_PROMPTS: usize = 20;
/// Sessions each window needs before a compaction rise is reported.
pub const MIN_SESSIONS: usize = 5;
/// Relative fall in activations per prompt that counts as a drop.
pub const DROP_RATIO: f64 = 0.3;
/// Relative rise in compactions per session that counts as a rise.
pub const RISE_RATIO: f64 = 0.5;
/// What the log cannot answer, printed under every summary.
pub const NOT_IN_LOG: &str = "Cache-hit rate and cost are not in the event log: `skillctl log usage` reads them from ccusage (Claude Code only); the OpenTelemetry backend has them per skill and agent (see `skillctl doctor`).";

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Window {
    pub from: String,
    pub to: String,
    pub events: usize,
    pub by_tool: BTreeMap<String, usize>,
    pub sessions: usize,
    pub prompts: usize,
    pub tool_calls: usize,
    pub activations: usize,
    pub activations_per_100_prompts: Option<f64>,
    pub skills: BTreeMap<String, usize>,
    pub subagent_runs: usize,
    pub agents: BTreeMap<String, usize>,
    pub compactions: usize,
    pub compactions_per_session: Option<f64>,
    pub mcp_calls: BTreeMap<String, usize>,
    /// Slash commands run, by name (logger 1.2.2+).
    pub commands: BTreeMap<String, usize>,
    pub test_commands_raw: usize,
    pub test_commands_wrapped: usize,
    /// Prompts for which the forced-eval hook injected a shortlist.
    pub hook_shortlists: usize,
    /// Shortlists followed, in the same session, by an activation of one of
    /// their candidates: activation through the hook.
    pub hook_followed: usize,
    pub hook_follow_ratio: Option<f64>,
    /// One entry per day of the window, oldest first, zeros included.
    pub daily: Vec<DayCounts>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct DayCounts {
    pub day: String,
    pub sessions: usize,
    pub prompts: usize,
    pub activations: usize,
    pub subagent_runs: usize,
    pub compactions: usize,
    pub shortlists: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Alert {
    pub check: &'static str,
    pub severity: &'static str,
    pub message: String,
    pub fix: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub days: u32,
    pub current: Window,
    pub previous: Window,
    pub alerts: Vec<Alert>,
    pub note: &'static str,
}

fn round(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// Compactions per session are small fractions, so they keep two decimals.
fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn window(events: &[Event], from: DateTime<Utc>, to: DateTime<Utc>) -> Window {
    let mut window = Window {
        from: from.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        to: to.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        ..Default::default()
    };
    let mut sessions = BTreeSet::new();
    let mut daily: BTreeMap<String, (DayCounts, BTreeSet<String>)> = BTreeMap::new();
    // One entry per calendar day the window touches, so charts have zeros.
    let mut day = from.date_naive();
    let last = (to - Duration::seconds(1)).date_naive();
    while day <= last {
        let key = day.to_string();
        daily.insert(
            key.clone(),
            (
                DayCounts {
                    day: key,
                    ..Default::default()
                },
                BTreeSet::new(),
            ),
        );
        day += Duration::days(1);
    }
    let mut shortlists: Vec<(String, String, Vec<String>)> = Vec::new();
    let mut seen_activations: Vec<(String, String, String)> = Vec::new();
    for event in events
        .iter()
        .filter(|event| event.time().is_some_and(|time| time >= from && time < to))
    {
        window.events += 1;
        *window.by_tool.entry(event.tool.clone()).or_default() += 1;
        let day_key = event.ts.get(..10).unwrap_or("").to_owned();
        let (counts, day_sessions) = daily.entry(day_key.clone()).or_insert_with(|| {
            (
                DayCounts {
                    day: day_key,
                    ..Default::default()
                },
                BTreeSet::new(),
            )
        });
        if let Some(session) = &event.session {
            sessions.insert(session.clone());
            day_sessions.insert(session.clone());
        }
        if let Some(command) = &event.command {
            *window.commands.entry(command.to_lowercase()).or_default() += 1;
        }
        match event.event.as_str() {
            "UserPromptSubmit" => {
                window.prompts += 1;
                counts.prompts += 1;
            }
            "SkillShortlist" => {
                window.hook_shortlists += 1;
                counts.shortlists += 1;
                shortlists.push((
                    event.session.clone().unwrap_or_default(),
                    event.ts.clone(),
                    event
                        .candidates
                        .clone()
                        .unwrap_or_default()
                        .iter()
                        .map(|c| c.to_lowercase())
                        .collect(),
                ));
            }
            "PostToolUse" => {
                window.tool_calls += 1;
                if let Some(skill) = &event.skill {
                    window.activations += 1;
                    counts.activations += 1;
                    *window.skills.entry(skill.to_lowercase()).or_default() += 1;
                    seen_activations.push((
                        event.session.clone().unwrap_or_default(),
                        event.ts.clone(),
                        skill.to_lowercase(),
                    ));
                }
                if let Some(server) = &event.server {
                    *window.mcp_calls.entry(server.clone()).or_default() += 1;
                }
                match event.test_command.as_deref() {
                    Some("raw") => window.test_commands_raw += 1,
                    Some("wrapped") => window.test_commands_wrapped += 1,
                    _ => {}
                }
            }
            "SubagentStop" => {
                if let Some(agent) = &event.agent {
                    window.subagent_runs += 1;
                    counts.subagent_runs += 1;
                    *window.agents.entry(agent.to_lowercase()).or_default() += 1;
                }
            }
            "PreCompact" => {
                window.compactions += 1;
                counts.compactions += 1;
            }
            _ => {}
        }
    }
    window.sessions = sessions.len();
    window.hook_followed = shortlists
        .iter()
        .filter(|(session, ts, candidates)| {
            seen_activations
                .iter()
                .any(|(s, t, skill)| s == session && t >= ts && candidates.contains(skill))
        })
        .count();
    if window.hook_shortlists > 0 {
        window.hook_follow_ratio = Some(round(
            window.hook_followed as f64 * 100.0 / window.hook_shortlists as f64,
        ));
    }
    window.daily = daily
        .into_iter()
        .map(|(_, (mut counts, day_sessions))| {
            counts.sessions = day_sessions.len();
            counts
        })
        .collect();
    if window.prompts > 0 {
        window.activations_per_100_prompts = Some(round(
            window.activations as f64 * 100.0 / window.prompts as f64,
        ));
    }
    if window.sessions > 0 {
        window.compactions_per_session =
            Some(round2(window.compactions as f64 / window.sessions as f64));
    }
    window
}

fn alerts(current: &Window, previous: &Window, days: u32) -> Vec<Alert> {
    let mut alerts = Vec::new();
    if current.events == 0 && previous.events > 0 {
        alerts.push(Alert {
            check: "log-silent",
            severity: "warning",
            message: format!(
                "no events in the last {days} days after {} in the {days} days before",
                previous.events
            ),
            fix: "Check `skillctl hook list` and `skillctl doctor`: the logger may be unregistered or the hooks not firing.".to_owned(),
        });
    }
    if let (Some(now), Some(before)) = (
        current.activations_per_100_prompts,
        previous.activations_per_100_prompts,
    ) {
        if current.prompts >= MIN_PROMPTS
            && previous.prompts >= MIN_PROMPTS
            && before > 0.0
            && now < before * (1.0 - DROP_RATIO)
        {
            alerts.push(Alert {
                check: "activation-drop",
                severity: "warning",
                message: format!(
                    "skill activations fell from {before} to {now} per 100 prompts ({} → {} prompts)",
                    previous.prompts, current.prompts
                ),
                fix: "Check for a harness upgrade (`skillctl doctor`), descriptions that stopped matching (`skillctl validate`), and whether the forced-eval hook is still registered.".to_owned(),
            });
        }
    }
    if let (Some(now), Some(before)) = (
        current.compactions_per_session,
        previous.compactions_per_session,
    ) {
        if current.sessions >= MIN_SESSIONS
            && previous.sessions >= MIN_SESSIONS
            && current.compactions >= 2
            && now > before * (1.0 + RISE_RATIO)
        {
            alerts.push(Alert {
                check: "compaction-rise",
                severity: "warning",
                message: format!(
                    "compactions rose from {before} to {now} per session ({} → {} sessions)",
                    previous.sessions, current.sessions
                ),
                fix: "Split long tasks with the `/checkpoint` command and check the context budget with `skillctl status --budget`.".to_owned(),
            });
        }
    }
    if current.test_commands_raw > 0 {
        alerts.push(Alert {
            check: "raw-test-commands",
            severity: "info",
            message: format!(
                "{} bare test or build command(s) ran next to {} wrapped one(s)",
                current.test_commands_raw, current.test_commands_wrapped
            ),
            fix: "Point AGENTS.md at the test-quiet wrapper; if agents keep bypassing it, register hooks/quiet-commands (`skillctl hook add quiet-commands`).".to_owned(),
        });
    }
    alerts
}

/// The summary of `events` for the `days` ending at `until` against the
/// `days` before that.
pub fn summarize(events: &[Event], until: DateTime<Utc>, days: u32) -> Summary {
    let span = Duration::days(i64::from(days.max(1)));
    let current = window(events, until - span, until);
    let previous = window(events, until - span - span, until - span);
    let alerts = alerts(&current, &previous, days.max(1));
    Summary {
        days: days.max(1),
        current,
        previous,
        alerts,
        note: NOT_IN_LOG,
    }
}

fn counts(map: &BTreeMap<String, usize>) -> String {
    map.iter()
        .map(|(name, count)| format!("{name} {count}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn per(value: Option<f64>) -> String {
    value.map_or_else(|| "–".to_owned(), |value| value.to_string())
}

fn day(stamp: &str) -> &str {
    stamp.get(..10).unwrap_or(stamp)
}

pub fn text(summary: &Summary) -> String {
    let (cur, prev) = (&summary.current, &summary.previous);
    let mut lines = vec![format!(
        "Log summary: {} → {} ({} days), previous {} → {}",
        day(&cur.from),
        day(&cur.to),
        summary.days,
        day(&prev.from),
        day(&prev.to)
    )];
    let row = |label: &str, now: usize, before: usize, extra: String| {
        format!(
            "  {label:<18}{now:>6} (prev {before}){}",
            if extra.is_empty() {
                String::new()
            } else {
                format!("   {extra}")
            }
        )
    };
    lines.push(row("events", cur.events, prev.events, counts(&cur.by_tool)));
    lines.push(row("sessions", cur.sessions, prev.sessions, String::new()));
    lines.push(row("prompts", cur.prompts, prev.prompts, String::new()));
    lines.push(row(
        "tool calls",
        cur.tool_calls,
        prev.tool_calls,
        String::new(),
    ));
    lines.push(row(
        "skill activations",
        cur.activations,
        prev.activations,
        format!(
            "{} per 100 prompts (prev {})",
            per(cur.activations_per_100_prompts),
            per(prev.activations_per_100_prompts)
        ),
    ));
    if !cur.skills.is_empty() {
        lines.push(format!("    {}", counts(&cur.skills)));
    }
    lines.push(row(
        "subagent runs",
        cur.subagent_runs,
        prev.subagent_runs,
        String::new(),
    ));
    if !cur.agents.is_empty() {
        lines.push(format!("    {}", counts(&cur.agents)));
    }
    lines.push(row(
        "compactions",
        cur.compactions,
        prev.compactions,
        format!(
            "{} per session (prev {})",
            per(cur.compactions_per_session),
            per(prev.compactions_per_session)
        ),
    ));
    let mcp_now: usize = cur.mcp_calls.values().sum();
    let mcp_before: usize = prev.mcp_calls.values().sum();
    lines.push(row(
        "MCP tool calls",
        mcp_now,
        mcp_before,
        counts(&cur.mcp_calls),
    ));
    let commands_now: usize = cur.commands.values().sum();
    let commands_before: usize = prev.commands.values().sum();
    lines.push(row(
        "slash commands",
        commands_now,
        commands_before,
        counts(&cur.commands),
    ));
    lines.push(format!(
        "  {:<18}raw {}, wrapped {} (prev raw {}, wrapped {})",
        "test commands",
        cur.test_commands_raw,
        cur.test_commands_wrapped,
        prev.test_commands_raw,
        prev.test_commands_wrapped
    ));
    lines.push(row(
        "hook shortlists",
        cur.hook_shortlists,
        prev.hook_shortlists,
        format!(
            "{} followed by an activation ({}%)",
            cur.hook_followed,
            per(cur.hook_follow_ratio)
        ),
    ));
    if summary.alerts.is_empty() {
        lines.push("Alerts: none".to_owned());
    } else {
        lines.push("Alerts:".to_owned());
        lines.extend(summary.alerts.iter().map(|alert| {
            format!(
                "  [{}] {}\n      fix: {}",
                alert.severity, alert.message, alert.fix
            )
        }));
    }
    lines.push(summary.note.to_owned());
    lines.join("\n")
}

fn parse_until(text: Option<&str>) -> Result<DateTime<Utc>, Error> {
    let Some(text) = text else {
        return Ok(DateTime::<Utc>::from(std::time::SystemTime::now()));
    };
    if let Some(time) = eventlog::parse_time(text) {
        return Ok(time);
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        if let Some(time) = date.and_hms_opt(0, 0, 0) {
            return Ok(time.and_utc());
        }
    }
    Err(Error::Usage {
        message: format!("--until {text:?} is not a time"),
        hint: "Use RFC 3339 (2026-10-20T00:00:00Z) or a day (2026-10-20).".into(),
    })
}

pub fn run(context: &mut Context<'_>, command: LogCommand) -> Result<(), Error> {
    match command {
        LogCommand::Usage { days } => super::usage::run(context, days),
        LogCommand::Summary {
            days,
            until,
            strict,
        } => {
            let until = parse_until(until.as_deref())?;
            let events = eventlog::read(&context.home.home().join("log/events.jsonl"));
            let summary = summarize(&events, until, days);
            context
                .output
                .result(&summary, &text(&summary))
                .map_err(super::output_error)?;
            let warnings = summary
                .alerts
                .iter()
                .filter(|alert| alert.severity == "warning")
                .count();
            if strict && warnings > 0 {
                return Err(Error::Doctor {
                    problems: warnings,
                    hint: "Investigate the alerts in the summary above.".into(),
                });
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(ts: &str, kind: &str, extra: &[(&str, &str)]) -> Event {
        let mut map = serde_json::Map::new();
        map.insert("ts".into(), ts.into());
        map.insert("tool".into(), "claude-code".into());
        map.insert("event".into(), kind.into());
        for (key, value) in extra {
            map.insert((*key).to_owned(), (*value).into());
        }
        serde_json::from_value(serde_json::Value::Object(map)).unwrap()
    }

    fn prompts(day: &str, session: &str, count: usize, activations: usize) -> Vec<Event> {
        let mut events = Vec::new();
        for i in 0..count {
            let ts = format!("{day}T{:02}:00:00Z", i % 24);
            events.push(event(&ts, "UserPromptSubmit", &[("session", session)]));
            if i < activations {
                events.push(event(
                    &ts,
                    "PostToolUse",
                    &[
                        ("session", session),
                        ("tool_name", "Skill"),
                        ("skill", "Review"),
                    ],
                ));
            }
        }
        events
    }

    #[test]
    fn windows_count_and_alerts_need_enough_data() {
        let until = eventlog::parse_time("2026-10-20T00:00:00Z").unwrap();
        // Previous week: 25 prompts, 10 activations. Current week: 25 prompts, 3.
        let mut events = prompts("2026-10-08", "p1", 25, 10);
        events.extend(prompts("2026-10-15", "c1", 25, 3));
        events.push(event(
            "2026-10-16T01:00:00Z",
            "SubagentStop",
            &[("session", "c1"), ("agent", "Explore")],
        ));
        events.push(event("2026-10-16T02:00:00Z", "SubagentStop", &[]));
        events.push(event(
            "2026-10-16T03:00:00Z",
            "PostToolUse",
            &[("tool_name", "Bash"), ("test_command", "raw")],
        ));
        events.push(event(
            "2026-10-16T04:00:00Z",
            "PreCompact",
            &[("session", "c1")],
        ));
        events.push(event("2026-10-21T00:00:00Z", "UserPromptSubmit", &[]));
        // Two shortlists: one followed by an activation of its candidate, one not.
        let mut shortlist = event(
            "2026-10-16T05:00:00Z",
            "SkillShortlist",
            &[("session", "c1")],
        );
        shortlist.candidates = Some(vec!["Review".into(), "triage".into()]);
        events.push(shortlist);
        events.push(event(
            "2026-10-16T05:01:00Z",
            "PostToolUse",
            &[
                ("session", "c1"),
                ("tool_name", "Skill"),
                ("skill", "review"),
            ],
        ));
        let mut ignored = event(
            "2026-10-16T06:00:00Z",
            "SkillShortlist",
            &[("session", "c2")],
        );
        ignored.candidates = Some(vec!["triage".into()]);
        events.push(ignored);
        let summary = summarize(&events, until, 7);
        assert_eq!(summary.current.hook_shortlists, 2);
        assert_eq!(summary.current.hook_followed, 1);
        assert_eq!(summary.current.hook_follow_ratio, Some(50.0));
        assert_eq!(
            summary.current.daily.len(),
            7,
            "one entry per day of the window"
        );
        assert_eq!(summary.current.daily[0].day, "2026-10-13");
        let day16 = summary
            .current
            .daily
            .iter()
            .find(|d| d.day == "2026-10-16")
            .unwrap();
        assert_eq!(
            (
                day16.shortlists,
                day16.compactions,
                day16.subagent_runs,
                day16.sessions
            ),
            (2, 1, 1, 2)
        );
        assert_eq!(
            summary.current.daily[2].prompts, 25,
            "2026-10-15 holds the prompts"
        );
        assert_eq!(summary.current.prompts, 25);
        assert_eq!(
            summary.current.activations, 4,
            "three prompt activations plus the followed shortlist"
        );
        assert_eq!(summary.current.activations_per_100_prompts, Some(16.0));
        assert_eq!(summary.previous.activations_per_100_prompts, Some(40.0));
        assert_eq!(summary.current.skills["review"], 4);
        assert_eq!(
            summary.current.subagent_runs, 1,
            "auxiliary agents are not runs"
        );
        assert_eq!(summary.current.agents["explore"], 1);
        assert_eq!(summary.current.compactions, 1);
        assert_eq!(summary.current.sessions, 2, "c1 and the shortlist-only c2");
        assert_eq!(summary.current.compactions_per_session, Some(0.5));
        assert_eq!(summary.previous.compactions_per_session, Some(0.0));
        assert_eq!(summary.current.test_commands_raw, 1);
        assert_eq!(summary.previous.events, 35);
        let checks: Vec<&str> = summary.alerts.iter().map(|a| a.check).collect();
        assert_eq!(checks, ["activation-drop", "raw-test-commands"]);
        let rendered = text(&summary);
        assert!(
            rendered.contains("4 (prev 10)   16 per 100 prompts (prev 40)"),
            "{rendered}"
        );
        assert!(
            rendered.contains("[warning] skill activations fell from 40 to 16 per 100 prompts"),
            "{rendered}"
        );
        assert!(
            rendered
                .contains("hook shortlists        2 (prev 0)   1 followed by an activation (50%)"),
            "{rendered}"
        );

        // Too few prompts: no drop alert, even with a worse ratio.
        let mut small = prompts("2026-10-08", "p1", 10, 8);
        small.extend(prompts("2026-10-15", "c1", 10, 0));
        assert!(summarize(&small, until, 7).alerts.is_empty());

        // A silent week after an active one.
        let silent = summarize(&prompts("2026-10-08", "p1", 5, 0), until, 7);
        assert_eq!(silent.alerts[0].check, "log-silent");

        // Compactions per session rising across enough sessions.
        let mut compact = Vec::new();
        for i in 0..6 {
            compact.push(event(
                "2026-10-08T01:00:00Z",
                "SessionStart",
                &[("session", &format!("p{i}"))],
            ));
            compact.push(event(
                "2026-10-15T01:00:00Z",
                "SessionStart",
                &[("session", &format!("c{i}"))],
            ));
            compact.push(event(
                "2026-10-15T02:00:00Z",
                "PreCompact",
                &[("session", &format!("c{i}"))],
            ));
        }
        let rising = summarize(&compact, until, 7);
        assert_eq!(rising.current.compactions_per_session, Some(1.0));
        assert_eq!(rising.alerts[0].check, "compaction-rise");
    }

    #[test]
    fn until_accepts_times_and_days() {
        assert_eq!(
            parse_until(Some("2026-10-20")).unwrap(),
            eventlog::parse_time("2026-10-20T00:00:00Z").unwrap()
        );
        assert_eq!(
            parse_until(Some("2026-10-20T12:30:00+02:00")).unwrap(),
            eventlog::parse_time("2026-10-20T10:30:00Z").unwrap()
        );
        assert!(parse_until(Some("yesterday")).is_err());
        assert!(parse_until(None).is_ok());
    }
}
