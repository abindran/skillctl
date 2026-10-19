use crate::{
    context::Context, error::Error, linker::normalize, registry::resolve::InstanceResolver, store,
};
use std::{io, path::Path};

/// Inspect without calling reconcile: status never repairs or writes links.
pub(super) fn health(
    context: &Context<'_>,
    path: &Path,
    want: &Path,
) -> Result<&'static str, Error> {
    match context.fs.symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok("missing"),
        Err(source) => Err(Error::Io {
            operation: format!("Inspect {}", path.display()),
            source,
            hint: "Check instance permissions.".into(),
        }),
        Ok(m) if m.file_type().is_symlink() => {
            let target = context.fs.read_link(path).map_err(|source| Error::Io {
                operation: format!("Read {}", path.display()),
                source,
                hint: "Check link permissions.".into(),
            })?;
            let target = normalize(&path.parent().expect("link parent").join(target));
            if target != normalize(want) {
                // Same ownership rule as reconcile: only links into the store or
                // drafts are ours to call wrong; anything else is a conflict.
                let managed = [context.home.store(), context.home.drafts()]
                    .iter()
                    .any(|root| {
                        let root = normalize(root);
                        target != root && target.starts_with(&root)
                    });
                return Ok(if managed { "wrong" } else { "conflict" });
            }
            match context.fs.symlink_metadata(want) {
                Ok(_) => Ok("ok"),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok("dangling"),
                Err(source) => Err(Error::Io {
                    operation: format!("Inspect {}", want.display()),
                    source,
                    hint: "Check store permissions.".into(),
                }),
            }
        }
        Ok(_) => Ok("conflict"),
    }
}
pub fn run(context: &mut Context<'_>, budget: bool) -> Result<(), Error> {
    super::init::ensure_home(context)?;
    let (mut value, mut text) = super::read_locked(context, || {
        let registry = super::load_registry(context)?;
        let resolver = InstanceResolver::new(&registry)?;
        let Some(found) = resolver.resolve(&super::attach::cwd()?)? else {
            return Ok((serde_json::json!({"attached":false}), "Not attached".into()));
        };
        let mut rows = Vec::new();
        let mut text = format!(
            "Project: {}\nInstance: {}",
            found.project_name,
            found.instance.path.display()
        );
        for (name, pin) in &found.project.skills {
            let want = store::store_dir(&context.home, name, pin);
            for target in &registry.settings.link_targets {
                let path = found
                    .instance
                    .path
                    .join(target.as_path())
                    .join(name.as_str());
                let state = health(context, &path, &want)?;
                rows.push(serde_json::json!({"skill":name,"pin":pin,"path":path,"target":want,"status":state}));
                text.push_str(&format!(
                    "\n  {name}@{pin} {}: {state}",
                    target.as_path().display()
                ));
            }
        }
        if !registry.global.skills.is_empty() {
            text.push_str("\nAlso loaded everywhere:");
            for (n, p) in &registry.global.skills {
                text.push_str(&format!("\n  {n}@{p}"));
            }
        }
        Ok((
            serde_json::json!({"attached":true,"project":found.project_name,"instance":found.instance.path,"skills":found.project.skills,"links":rows,"global":registry.global.skills}),
            text,
        ))
    })?;
    if budget {
        // What the agent sees here: project pins (if attached) plus globals.
        let registry = super::read_locked(context, || super::load_registry(context))?;
        let mut visible = registry.global.skills.clone();
        if let Some(name) = value["project"].as_str() {
            let name: crate::names::ProjectName = name.parse()?;
            visible.extend(registry.projects[&name].skills.clone());
        }
        let index = super::advisory::index(context, &visible)?;
        text.push_str("\nSkill index:");
        for entry in &index.skills {
            text.push_str(&format!(
                "\n  {}@{}: {} chars, ~{} tokens{}",
                entry.name,
                entry.pin,
                entry.chars,
                entry.tokens,
                if entry.truncated {
                    format!(
                        " (description {} chars; truncated at {})",
                        entry.description_chars,
                        super::advisory::DESCRIPTION_MAX_CHARS
                    )
                } else {
                    String::new()
                }
            ));
        }
        text.push_str(&format!(
            "\n  total ~{} / {} tokens ({}% of a {}-token context window){}",
            index.tokens,
            index.budget_tokens,
            super::advisory::INDEX_SHARE_PERCENT,
            super::advisory::CONTEXT_WINDOW_TOKENS,
            if index.over_budget {
                " — O143 warning: over budget; skills past the cap lose their description"
            } else {
                ""
            }
        ));
        value["budget"] = serde_json::to_value(&index).map_err(|error| Error::Io {
            operation: "Serialize budget".into(),
            source: std::io::Error::other(error),
            hint: "Report this as a skillctl bug.".into(),
        })?;
    }
    context
        .output
        .result(&value, &text)
        .map_err(super::output_error)
}
