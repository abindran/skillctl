//! `artifacts [--unused]`: every store version with its kind, where it is
//! pinned, and what the event log says about its use, for the periodic bloat
//! review. `--unused` keeps only versions nothing pins, and pinned skills or
//! agents with no recorded use after a week of logging.
use crate::eventlog;
use crate::{
    artifact::Kind,
    context::Context,
    error::Error,
    names::SkillName,
    registry::{Pin, Registry},
    store::{self, meta::Metadata},
};
use semver::Version;
use serde::Serialize;
use std::collections::BTreeMap;

/// Days of log history before "no recorded use" counts as unused.
pub const UNUSED_AFTER_DAYS: i64 = 7;

#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub name: SkillName,
    pub version: Version,
    pub kind: Kind,
    /// Project names pinning this version, plus `global` or `hook`.
    pub pinned_by: Vec<String>,
    /// Recorded uses: skill activations (`PostToolUse` of `Skill`) or
    /// subagent runs (`SubagentStop` by `agent_type`); `None` for kinds the
    /// log cannot see (hooks, rules, commands, scripts).
    pub uses: Option<usize>,
    pub last_used: Option<String>,
    pub unused: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Default)]
struct Usage {
    skills: BTreeMap<String, (usize, String)>,
    agents: BTreeMap<String, (usize, String)>,
    span_days: i64,
}

fn usage(log: &std::path::Path) -> Usage {
    let events = eventlog::read(log);
    let mut usage = Usage {
        span_days: eventlog::span_days(&events),
        ..Default::default()
    };
    let bump = |map: &mut BTreeMap<String, (usize, String)>, key: String, ts: &str| {
        let entry = map.entry(key).or_insert((0, ts.to_owned()));
        entry.0 += 1;
        if ts > entry.1.as_str() {
            entry.1 = ts.to_owned();
        }
    };
    for event in &events {
        match event.event.as_str() {
            "PostToolUse" => {
                if let Some(skill) = &event.skill {
                    bump(&mut usage.skills, skill.to_lowercase(), &event.ts);
                }
            }
            "SubagentStop" => {
                if let Some(agent) = &event.agent {
                    bump(&mut usage.agents, agent.to_lowercase(), &event.ts);
                }
            }
            _ => {}
        }
    }
    usage
}

fn pinned_by(registry: &Registry, name: &SkillName, version: &Version) -> Vec<String> {
    let pin = Pin::Version(version.clone());
    let mut by: Vec<String> = registry
        .projects
        .iter()
        .filter(|(_, project)| project.skills.get(name) == Some(&pin))
        .map(|(project, _)| project.to_string())
        .collect();
    if registry.global.skills.get(name) == Some(&pin) {
        by.push("global".into());
    }
    if registry.global.hooks.get(name) == Some(&pin) {
        by.push("hook".into());
    }
    by
}

pub fn rows(context: &Context<'_>, registry: &Registry) -> Result<Vec<Row>, Error> {
    let fs = context.fs.as_ref();
    let usage = usage(&context.home.home().join("log/events.jsonl"));
    let mut rows = Vec::new();
    for name in store::list_skills(fs, &context.home)? {
        for version in store::list_versions(fs, &context.home, &name)? {
            let kind = Metadata::load(
                fs,
                &context.home.version_dir(&name, &version).join("meta.json"),
            )
            .map(|meta| meta.kind)
            .unwrap_or_default();
            let pinned = pinned_by(registry, &name, &version);
            let seen = match kind {
                Kind::Skill => Some(usage.skills.get(&name.as_str().to_lowercase())),
                Kind::Agent => Some(usage.agents.get(&name.as_str().to_lowercase())),
                _ => None,
            };
            let (uses, last_used) = match seen {
                Some(Some((count, last))) => (Some(*count), Some(last.clone())),
                Some(None) => (Some(0), None),
                None => (None, None),
            };
            let (unused, reason) = if pinned.is_empty() {
                (true, Some("no project, global or hook pin".to_owned()))
            } else if uses == Some(0) && usage.span_days >= UNUSED_AFTER_DAYS {
                (
                    true,
                    Some(format!(
                        "pinned but no recorded use in {} days of log",
                        usage.span_days
                    )),
                )
            } else {
                (false, None)
            };
            rows.push(Row {
                name: name.clone(),
                version,
                kind,
                pinned_by: pinned,
                uses,
                last_used,
                unused,
                reason,
            });
        }
    }
    Ok(rows)
}

pub fn run(context: &mut Context<'_>, unused: bool) -> Result<(), Error> {
    super::init::ensure_home(context)?;
    let mut rows = super::read_locked(context, || {
        let registry = super::load_registry(context)?;
        rows(context, &registry)
    })?;
    if unused {
        rows.retain(|row| row.unused);
    }
    let text = if rows.is_empty() {
        if unused {
            "Nothing unused.".to_owned()
        } else {
            "No artifacts in the store.".to_owned()
        }
    } else {
        rows.iter()
            .map(|row| {
                format!(
                    "{}@{} ({}) — {}{}{}",
                    row.name,
                    row.version,
                    row.kind,
                    if row.pinned_by.is_empty() {
                        "unpinned".to_owned()
                    } else {
                        format!("pinned by {}", row.pinned_by.join(", "))
                    },
                    match (row.uses, &row.last_used) {
                        (Some(0), _) => "; no recorded use".to_owned(),
                        (Some(n), Some(last)) => format!("; {n} use(s), last {last}"),
                        (Some(n), None) => format!("; {n} use(s)"),
                        (None, _) => String::new(),
                    },
                    match &row.reason {
                        Some(reason) => format!("\n  unused: {reason}"),
                        None => String::new(),
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    context
        .output
        .result(&rows, &text)
        .map_err(super::output_error)
}
