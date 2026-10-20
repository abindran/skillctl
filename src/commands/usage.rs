//! `log usage [--days N]`: Claude Code token and cost figures from
//! `ccusage daily --json`, joined per day with the event log so cost and
//! output tokens can be read per prompt. ccusage reads Claude Code's local
//! session files and is optional: its absence is reported, never fatal.
//! Cursor publishes no comparable data, so these figures are Claude Code's.
use crate::{
    context::Context,
    error::Error,
    eventlog::{self, Event},
};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    process::{Command, Stdio},
    time::{Duration as StdDuration, Instant},
};

/// How long ccusage may take; it parses every session file it can find.
pub const CCUSAGE_TIMEOUT: StdDuration = StdDuration::from_secs(90);

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ModelCost {
    pub model: String,
    pub cost: f64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Day {
    pub date: String,
    /// Claude Code prompts that day, from the event log.
    pub prompts: usize,
    pub sessions: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub total_tokens: u64,
    pub cost: f64,
    /// Cache reads over everything that went in: reads, fresh input and cache writes.
    pub cache_hit_ratio: Option<f64>,
    pub cost_per_prompt: Option<f64>,
    pub output_tokens_per_prompt: Option<f64>,
    pub models: Vec<ModelCost>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Usage {
    pub available: bool,
    pub note: String,
    pub from: String,
    pub to: String,
    pub days: Vec<Day>,
    pub total_cost: f64,
    pub total_output_tokens: u64,
    pub prompts: usize,
    pub cache_hit_ratio: Option<f64>,
    pub cost_per_prompt: Option<f64>,
    pub output_tokens_per_prompt: Option<f64>,
}

fn u64_of(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn f64_of(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn round(value: f64, places: i32) -> f64 {
    let factor = 10f64.powi(places);
    (value * factor).round() / factor
}

fn ratio(part: u64, whole: u64) -> Option<f64> {
    (whole > 0).then(|| round(part as f64 / whole as f64, 3))
}

/// The per-day rows of `ccusage daily --json`: `daily[]` with `period` (or
/// `date`), token counts, `totalCost` and `modelBreakdowns`.
pub fn parse_ccusage(json: &Value) -> Vec<Day> {
    let Some(rows) = json.get("daily").and_then(Value::as_array) else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            let date = row
                .get("period")
                .or_else(|| row.get("date"))
                .and_then(Value::as_str)?;
            let models = row
                .get("modelBreakdowns")
                .and_then(Value::as_array)
                .map(|models| {
                    models
                        .iter()
                        .filter_map(|model| {
                            Some(ModelCost {
                                model: model.get("modelName")?.as_str()?.to_owned(),
                                cost: round(f64_of(model, "cost"), 4),
                                output_tokens: u64_of(model, "outputTokens"),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(Day {
                date: date.to_owned(),
                input_tokens: u64_of(row, "inputTokens"),
                output_tokens: u64_of(row, "outputTokens"),
                cache_creation_tokens: u64_of(row, "cacheCreationTokens"),
                cache_read_tokens: u64_of(row, "cacheReadTokens"),
                total_tokens: u64_of(row, "totalTokens"),
                cost: round(f64_of(row, "totalCost"), 4),
                models,
                ..Default::default()
            })
        })
        .collect()
}

/// Keep the days in `[from, to)`, count that day's Claude Code prompts and
/// sessions from the log, and derive the per-prompt figures.
pub fn join(days: Vec<Day>, events: &[Event], from: NaiveDate, to: NaiveDate) -> Vec<Day> {
    let mut prompts: BTreeMap<String, usize> = BTreeMap::new();
    let mut sessions: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for event in events.iter().filter(|event| event.tool == "claude-code") {
        let day = event.ts.get(..10).unwrap_or("").to_owned();
        if event.event == "UserPromptSubmit" {
            *prompts.entry(day.clone()).or_default() += 1;
        }
        if let Some(session) = &event.session {
            sessions.entry(day).or_default().insert(session.clone());
        }
    }
    let mut days: Vec<Day> = days
        .into_iter()
        .filter(|day| {
            NaiveDate::parse_from_str(&day.date, "%Y-%m-%d")
                .map(|date| date >= from && date < to)
                .unwrap_or(false)
        })
        .map(|mut day| {
            day.prompts = prompts.get(&day.date).copied().unwrap_or(0);
            day.sessions = sessions.get(&day.date).map_or(0, BTreeSet::len);
            day.cache_hit_ratio = ratio(
                day.cache_read_tokens,
                day.cache_read_tokens + day.input_tokens + day.cache_creation_tokens,
            );
            if day.prompts > 0 {
                day.cost_per_prompt = Some(round(day.cost / day.prompts as f64, 3));
                day.output_tokens_per_prompt =
                    Some(round(day.output_tokens as f64 / day.prompts as f64, 0));
            }
            day
        })
        .collect();
    days.sort_by(|a, b| a.date.cmp(&b.date));
    days
}

pub fn summarize(
    days: Vec<Day>,
    from: NaiveDate,
    to: NaiveDate,
    available: bool,
    note: String,
) -> Usage {
    let total_cost = round(days.iter().map(|day| day.cost).sum(), 2);
    let total_output_tokens = days.iter().map(|day| day.output_tokens).sum();
    let prompts = days.iter().map(|day| day.prompts).sum();
    let reads: u64 = days.iter().map(|day| day.cache_read_tokens).sum();
    let went_in: u64 = days
        .iter()
        .map(|day| day.cache_read_tokens + day.input_tokens + day.cache_creation_tokens)
        .sum();
    Usage {
        available,
        note,
        from: from.to_string(),
        to: to.to_string(),
        cache_hit_ratio: ratio(reads, went_in),
        cost_per_prompt: (prompts > 0).then(|| round(total_cost / prompts as f64, 3)),
        output_tokens_per_prompt: (prompts > 0)
            .then(|| round(total_output_tokens as f64 / prompts as f64, 0)),
        days,
        total_cost,
        total_output_tokens,
        prompts,
    }
}

/// `ccusage daily --json --since <YYYYMMDD>` with a timeout; `None` when
/// ccusage is missing, fails or hangs. stdout is drained on a thread so a
/// large report cannot stall the child on a full pipe.
fn ccusage(since: &str) -> Option<String> {
    let mut child = Command::new("ccusage")
        .args(["daily", "--json", "--since", since])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return None,
            Ok(None) if started.elapsed() < CCUSAGE_TIMEOUT => {
                std::thread::sleep(StdDuration::from_millis(100));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    reader.join().ok()
}

fn tokens(value: u64) -> String {
    match value {
        v if v >= 1_000_000 => format!("{:.2}M", v as f64 / 1e6),
        v if v >= 1_000 => format!("{:.1}K", v as f64 / 1e3),
        v => v.to_string(),
    }
}

fn percent(value: Option<f64>) -> String {
    value.map_or_else(|| "–".to_owned(), |v| format!("{:.1}%", v * 100.0))
}

pub fn text(usage: &Usage) -> String {
    if !usage.available {
        return format!("Usage: {}", usage.note);
    }
    let mut lines = vec![format!(
        "Usage (Claude Code, via ccusage): {} → {}",
        usage.from, usage.to
    )];
    lines.push(format!(
        "  {:<12}{:>8}{:>12}{:>11}{:>10}{:>13}",
        "date", "prompts", "output tok", "cache hit", "cost", "cost/prompt"
    ));
    for day in &usage.days {
        lines.push(format!(
            "  {:<12}{:>8}{:>12}{:>11}{:>10}{:>13}",
            day.date,
            day.prompts,
            tokens(day.output_tokens),
            percent(day.cache_hit_ratio),
            format!("${:.2}", day.cost),
            day.cost_per_prompt
                .map_or_else(|| "–".to_owned(), |v| format!("${v:.2}")),
        ));
    }
    lines.push(format!(
        "  {:<12}{:>8}{:>12}{:>11}{:>10}{:>13}",
        "total",
        usage.prompts,
        tokens(usage.total_output_tokens),
        percent(usage.cache_hit_ratio),
        format!("${:.2}", usage.total_cost),
        usage
            .cost_per_prompt
            .map_or_else(|| "–".to_owned(), |v| format!("${v:.2}")),
    ));
    lines.push(usage.note.clone());
    lines.join("\n")
}

pub fn run(context: &mut Context<'_>, days: u32) -> Result<(), Error> {
    let until: DateTime<Utc> = DateTime::<Utc>::from(std::time::SystemTime::now());
    let to = until.date_naive() + Duration::days(1);
    let from = to - Duration::days(i64::from(days.max(1)));
    let events = eventlog::read(&context.home.home().join("log/events.jsonl"));
    let usage = match ccusage(&from.format("%Y%m%d").to_string()) {
        Some(text) => {
            let parsed = serde_json::from_str::<Value>(&text)
                .map(|json| parse_ccusage(&json))
                .unwrap_or_default();
            summarize(
                join(parsed, &events, from, to),
                from,
                to,
                true,
                "Cost and tokens come from ccusage (Claude Code's local session files); prompts and sessions from the skillctl event log. Cursor publishes no comparable data.".to_owned(),
            )
        }
        None => summarize(
            Vec::new(),
            from,
            to,
            false,
            "ccusage is not on PATH or produced no output. Install it with `npm install -g ccusage`; `skillctl doctor` checks for it.".to_owned(),
        ),
    };
    context
        .output
        .result(&usage, &text(&usage))
        .map_err(super::output_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Value {
        serde_json::json!({
            "daily": [
                {"period": "2026-10-18", "inputTokens": 1000, "outputTokens": 50000, "cacheCreationTokens": 9000, "cacheReadTokens": 90000, "totalTokens": 150000, "totalCost": 12.5,
                 "modelBreakdowns": [{"modelName": "claude-opus-5-5", "cost": 12.0, "outputTokens": 48000}, {"modelName": "claude-haiku-4-5", "cost": 0.5, "outputTokens": 2000}]},
                {"period": "2026-10-19", "inputTokens": 0, "outputTokens": 10000, "cacheCreationTokens": 0, "cacheReadTokens": 0, "totalTokens": 10000, "totalCost": 2.0},
                {"period": "2026-10-25", "inputTokens": 1, "outputTokens": 1, "cacheCreationTokens": 1, "cacheReadTokens": 1, "totalTokens": 4, "totalCost": 1.0}
            ],
            "totals": {"totalCost": 15.5}
        })
    }

    #[test]
    fn ccusage_days_are_parsed_joined_with_prompts_and_summarised() {
        let days = parse_ccusage(&fixture());
        assert_eq!(days.len(), 3);
        assert_eq!(days[0].models.len(), 2);
        assert_eq!(days[0].models[0].model, "claude-opus-5-5");
        let events = eventlog::parse(concat!(
            r#"{"ts":"2026-10-18T09:00:00Z","tool":"claude-code","event":"UserPromptSubmit","session":"a"}"#,
            "\n",
            r#"{"ts":"2026-10-18T10:00:00Z","tool":"claude-code","event":"UserPromptSubmit","session":"a"}"#,
            "\n",
            r#"{"ts":"2026-10-18T11:00:00Z","tool":"claude-code","event":"UserPromptSubmit","session":"b"}"#,
            "\n",
            r#"{"ts":"2026-10-18T11:00:00Z","tool":"claude-code","event":"UserPromptSubmit","session":"c"}"#,
            "\n",
            r#"{"ts":"2026-10-18T12:00:00Z","tool":"claude-code","event":"UserPromptSubmit","session":"c"}"#,
            "\n",
            r#"{"ts":"2026-10-18T13:00:00Z","tool":"cursor","event":"UserPromptSubmit","session":"x"}"#,
            "\n",
            r#"{"ts":"2026-10-19T09:00:00Z","tool":"claude-code","event":"Stop","session":"d"}"#,
            "\n",
        ));
        let from = NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 10, 21).unwrap();
        let joined = join(days, &events, from, to);
        assert_eq!(joined.len(), 2, "the day after the window is dropped");
        assert_eq!(joined[0].prompts, 5, "Cursor prompts do not count");
        assert_eq!(joined[0].sessions, 3);
        assert_eq!(joined[0].cache_hit_ratio, Some(0.9));
        assert_eq!(joined[0].cost_per_prompt, Some(2.5));
        assert_eq!(joined[0].output_tokens_per_prompt, Some(10000.0));
        assert_eq!(joined[1].prompts, 0);
        assert_eq!(joined[1].cost_per_prompt, None);
        assert_eq!(joined[1].cache_hit_ratio, None);
        let usage = summarize(joined, from, to, true, "note".into());
        assert_eq!(usage.total_cost, 14.5);
        assert_eq!(usage.total_output_tokens, 60000);
        assert_eq!(usage.prompts, 5);
        assert_eq!(usage.cost_per_prompt, Some(2.9));
        assert_eq!(usage.cache_hit_ratio, Some(0.9));
        let rendered = text(&usage);
        assert!(
            rendered.contains("2026-10-18         5       50.0K      90.0%    $12.50        $2.50"),
            "{rendered}"
        );
        assert!(
            rendered.contains("total              5       60.0K      90.0%    $14.50        $2.90"),
            "{rendered}"
        );
        let missing = summarize(Vec::new(), from, to, false, "ccusage is not on PATH".into());
        assert_eq!(text(&missing), "Usage: ccusage is not on PATH");
        assert!(parse_ccusage(&serde_json::json!({"nope": 1})).is_empty());
    }
}
