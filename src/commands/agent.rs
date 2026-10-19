//! `agent derive <name> --model <id>`: write the Cursor override `cursor.md`
//! into an agent draft. Cursor imports `.claude/agents/*.md` itself and
//! understands only `name`, `description`, `model`, `readonly` and
//! `is_background`, so an override is needed exactly when the model is a
//! Claude Code alias or skills must be preloaded; it keeps the agent's prompt
//! verbatim and prepends a line per preloaded skill telling the agent to read
//! that skill's `SKILL.md`.
use crate::{
    artifact::Kind, cli::AgentCommand, context::Context, error::Error,
    linker::AGENT_CURSOR_OVERRIDE, names::SkillName, skill::Skill, validate::agent as rules,
};
use serde_yaml_ng::Value;

/// Build the override document from a parsed `AGENT.md`.
pub fn derive(agent: &Skill, cursor_model: &str) -> Result<String, Error> {
    let name = agent.frontmatter.name.value().cloned().unwrap_or_default();
    let description = agent
        .frontmatter
        .description
        .value()
        .cloned()
        .unwrap_or_default();
    let mut map = serde_yaml_ng::Mapping::new();
    map.insert(Value::from("name"), Value::from(name));
    map.insert(Value::from("description"), Value::from(description));
    map.insert(Value::from("model"), Value::from(cursor_model));
    if let Some(readonly) = agent.frontmatter.extra.get("readonly") {
        map.insert(Value::from("readonly"), readonly.clone());
    }
    if let Some(background) = agent.frontmatter.extra.get("background") {
        map.insert(Value::from("is_background"), background.clone());
    }
    let yaml =
        serde_yaml_ng::to_string(&Value::Mapping(map)).map_err(|error| Error::Validation {
            summary: format!("Cannot serialise the override frontmatter: {error}"),
            hint: "Check the AGENT.md frontmatter values.".into(),
        })?;
    let mut body = String::new();
    let skills = rules::skills(agent);
    if !skills.is_empty() {
        body.push_str(
            "Before starting, read these skill files in full and follow them; Cursor does not preload them:\n",
        );
        for skill in &skills {
            body.push_str(&format!("- .claude/skills/{skill}/SKILL.md\n"));
        }
        body.push('\n');
    }
    body.push_str(agent.body.trim_start_matches('\n'));
    if !body.ends_with('\n') {
        body.push('\n');
    }
    Ok(format!("---\n{yaml}---\n\n{body}"))
}

pub fn run(context: &mut Context<'_>, command: AgentCommand) -> Result<(), Error> {
    let AgentCommand::Derive { agent, model } = command;
    let name: SkillName = agent.parse()?;
    super::pins::write_locked(context, |context| {
        let draft = context.home.draft_dir(&name);
        if Kind::detect(context.fs.as_ref(), &draft)? != Some(Kind::Agent) {
            return Err(super::attach::usage(format!(
                "No agent draft '{name}' at {}; run skillctl edit {name} (or new --kind agent {name}) first",
                draft.display()
            )));
        }
        let parsed = Skill::load_kind(context.fs.as_ref(), &draft, Kind::Agent)?;
        let document = derive(&parsed, &model)?;
        let path = draft.join(AGENT_CURSOR_OVERRIDE);
        if !context.flags.dry_run {
            context
                .fs
                .write_atomic(&path, document.as_bytes())
                .map_err(|source| Error::Io {
                    operation: format!("Write {}", path.display()),
                    source,
                    hint: "Check the draft folder permissions.".into(),
                })?;
            context
                .fs
                .set_mode(&path, 0o644)
                .map_err(|source| Error::Io {
                    operation: format!("Set mode on {}", path.display()),
                    source,
                    hint: "Check the draft folder permissions.".into(),
                })?;
        }
        let text = format!(
            "{}{} for {name} with model {model}\n  seal the draft and re-add it to link .cursor/agents/{name}.md",
            if context.flags.dry_run {
                "Would write "
            } else {
                "Wrote "
            },
            path.display()
        );
        context
            .output
            .result(
                &serde_json::json!({"agent": name, "path": path, "model": model, "dry_run": context.flags.dry_run}),
                &text,
            )
            .map_err(super::output_error)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_keeps_the_prompt_and_prepends_preload_instructions() {
        let agent = Skill::parse_kind(
            "/tmp/repo-explorer",
            "---\nname: repo-explorer\ndescription: Read-only investigation.\ntools: Read, Grep, Glob\nmodel: haiku\nreadonly: true\nskills: [project-architecture, conventions]\nmaxTurns: 15\nmetadata:\n  version: '1.0.0'\n---\nYou investigate; you never edit.\nReturn ONLY JSON.\n",
            Kind::Agent,
        )
        .unwrap();
        let text = derive(&agent, "gpt-5-mini").unwrap();
        assert!(text.starts_with("---\nname: repo-explorer\n"), "{text}");
        assert!(text.contains("model: gpt-5-mini\n"));
        assert!(text.contains("readonly: true\n"));
        assert!(!text.contains("tools:") && !text.contains("maxTurns"));
        assert!(text.contains("- .claude/skills/project-architecture/SKILL.md\n- .claude/skills/conventions/SKILL.md\n"));
        assert!(text.ends_with("You investigate; you never edit.\nReturn ONLY JSON.\n"));
        // The override passes H226 against its source.
        let parsed = Skill::parse_kind("/tmp/repo-explorer", &text, Kind::Agent).unwrap();
        assert!(parsed.body.contains(agent.body.trim()));
    }
}
