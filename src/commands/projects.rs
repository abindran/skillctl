use crate::{context::Context, error::Error, names::ProjectName, registry::Project};
use serde::Serialize;
use std::{io, path::PathBuf};

#[derive(Debug, Serialize)]
pub struct ProjectListing {
    pub name: ProjectName,
    #[serde(flatten)]
    pub project: Project,
    pub missing_instances: Vec<PathBuf>,
}

pub fn run(context: &mut Context<'_>) -> Result<(), Error> {
    super::init::ensure_home(context)?;
    let rows = super::read_locked(context, || {
        let registry = super::load_registry(context)?;
        let mut rows = Vec::new();
        for (name, project) in registry.projects {
            let mut missing_instances = Vec::new();
            for instance in &project.instances {
                match context.fs.symlink_metadata(&instance.path) {
                    Ok(info) if info.is_dir() => {}
                    Ok(_) => missing_instances.push(instance.path.clone()),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        missing_instances.push(instance.path.clone())
                    }
                    Err(source) => {
                        return Err(Error::Io {
                            operation: format!("Inspect instance {}", instance.path.display()),
                            source,
                            hint: "Check the instance folder's permissions.".into(),
                        });
                    }
                }
            }
            rows.push(ProjectListing {
                name,
                project,
                missing_instances,
            });
        }
        Ok(rows)
    })?;
    context
        .output
        .result(&rows, &text(&rows))
        .map_err(super::output_error)
}

pub fn text(rows: &[ProjectListing]) -> String {
    if rows.is_empty() {
        return "No projects.".into();
    }
    rows.iter()
        .map(|row| {
            let mut text = format!(
                "{} ({} skills, {} instances)",
                row.name,
                row.project.skills.len(),
                row.project.instances.len()
            );
            for instance in &row.project.instances {
                text.push_str(&format!(
                    "\n  {}{}",
                    instance.path.display(),
                    if row.missing_instances.contains(&instance.path) {
                        " [missing]"
                    } else {
                        ""
                    }
                ));
            }
            text
        })
        .collect::<Vec<_>>()
        .join("\n")
}
