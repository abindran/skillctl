use crate::{context::Context, error::Error, registry::resolve::InstanceResolver, store};
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
            let target = path.parent().expect("link parent").join(target);
            if target != want {
                return Ok("wrong");
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
pub fn run(context: &mut Context<'_>) -> Result<(), Error> {
    super::init::ensure_home(context)?;
    let (value, text) = super::read_locked(context, || {
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
    context
        .output
        .result(&value, &text)
        .map_err(super::output_error)
}
