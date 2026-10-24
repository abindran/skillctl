//! Read and change `settings` keys in the registry. Changing link targets offers
//! to relink everything so links in dropped targets are cleaned up.
use crate::{
    context::Context,
    error::Error,
    paths::{HomePath, RelPath},
    registry::Settings,
};

const KEYS: [&str; 5] = [
    "link_targets",
    "git_exclude",
    "global_link_targets",
    "global_in_instances",
    "validate.allow",
];

fn unknown_key(key: &str) -> Error {
    super::attach::usage(format!(
        "Unknown setting '{key}'; valid keys: {}",
        KEYS.join(", ")
    ))
}

fn value_of(settings: &Settings, key: &str) -> Result<(serde_json::Value, String), Error> {
    let list = |items: Vec<String>| (serde_json::json!(items), items.join(","));
    Ok(match key {
        "link_targets" => list(
            settings
                .link_targets
                .iter()
                .map(|t| t.as_path().display().to_string())
                .collect(),
        ),
        "global_link_targets" => list(
            settings
                .global_link_targets
                .iter()
                .map(|t| t.as_path().display().to_string())
                .collect(),
        ),
        "validate.allow" => list(settings.validate.allow.clone()),
        "git_exclude" => (
            serde_json::json!(settings.git_exclude),
            settings.git_exclude.to_string(),
        ),
        "global_in_instances" => (
            serde_json::json!(settings.global_in_instances),
            settings.global_in_instances.to_string(),
        ),
        _ => return Err(unknown_key(key)),
    })
}

fn items(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

fn boolean(key: &str, value: &str) -> Result<bool, Error> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Ok(true),
        "false" | "no" | "off" | "0" => Ok(false),
        _ => Err(super::attach::usage(format!(
            "{key} must be true or false, not {value:?}"
        ))),
    }
}

pub fn get(context: &mut Context<'_>, key: &str) -> Result<(), Error> {
    super::init::ensure_home(context)?;
    let (value, text) = super::read_locked(context, || {
        let registry = super::load_registry_or_default(context)?;
        value_of(&registry.settings, key)
    })?;
    context
        .output
        .result(&serde_json::json!({"key": key, "value": value}), &text)
        .map_err(super::output_error)
}

pub fn set(context: &mut Context<'_>, key: &str, value: &str) -> Result<(), Error> {
    super::pins::write_locked(context, |context| {
        let mut registry = super::load_registry(context)?;
        let settings = &mut registry.settings;
        let mut relink = false;
        match key {
            "link_targets" => {
                let targets = items(value)
                    .into_iter()
                    .map(RelPath::new)
                    .collect::<Result<Vec<_>, _>>()?;
                if targets.is_empty() {
                    return Err(super::attach::usage(
                        "link_targets needs at least one folder, for example .claude/skills",
                    ));
                }
                // Remember dropped targets so relink --all can clean their managed links.
                for old in &settings.link_targets {
                    if !targets.contains(old) && !settings.previous_link_targets.contains(old) {
                        settings.previous_link_targets.push(old.clone());
                    }
                }
                relink = settings.link_targets != targets;
                settings.link_targets = targets;
            }
            "global_link_targets" => {
                let targets = items(value)
                    .into_iter()
                    .map(HomePath::new)
                    .collect::<Result<Vec<_>, _>>()?;
                relink = settings.global_link_targets != targets;
                settings.global_link_targets = targets;
            }
            "git_exclude" => settings.git_exclude = boolean(key, value)?,
            "global_in_instances" => {
                let enabled = boolean(key, value)?;
                relink = settings.global_in_instances != enabled;
                settings.global_in_instances = enabled;
            }
            "validate.allow" => settings.validate.allow = items(value),
            _ => return Err(unknown_key(key)),
        }
        registry.validate()?;
        if !context.flags.dry_run {
            registry.save(context.fs.as_ref(), &context.home.registry())?;
        }
        let (json_value, text_value) = value_of(&registry.settings, key)?;
        let mut text = format!(
            "{}{key} = {text_value}",
            if context.flags.dry_run {
                "Would set "
            } else {
                "Set "
            }
        );
        let mut reports = Vec::new();
        let mut relinked = false;
        if relink {
            context.prompt.set_answer_flag("--yes");
            let answer = match context
                .prompt
                .confirm("Run skillctl relink --all now to apply the change?", true)
            {
                Ok(answer) => answer,
                Err(Error::MissingAnswer { .. }) => {
                    text.push_str(
                        "\nRun skillctl relink --all to apply the change to existing links.",
                    );
                    false
                }
                Err(error) => return Err(error),
            };
            if answer {
                reports = super::pins::reconcile_everything(context, &mut registry)?;
                relinked = true;
                for report in &reports {
                    for link in &report.links {
                        text.push_str(&format!("\n  {:?} {}", link.status, link.path.display()));
                    }
                }
            }
        }
        context
            .output
            .result(
                &serde_json::json!({
                    "key": key,
                    "value": json_value,
                    "relinked": relinked,
                    "instances": reports,
                    "dry_run": context.flags.dry_run,
                }),
                &text,
            )
            .map_err(super::output_error)?;
        super::attach::conflicts(
            reports
                .iter()
                .any(crate::linker::ReconcileReport::has_conflicts),
        )
    })
}
