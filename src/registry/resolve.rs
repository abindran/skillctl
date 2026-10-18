//! Resolve folders by filesystem identity, never by textual path prefixes.
//!
//! Build one resolver per command while holding the registry lock. Instance
//! identities are read once; missing instances are left for doctor to report.
//! Like the file-identity helpers, this implementation targets Unix.
use std::{collections::HashMap, io, path::Path};

use crate::{
    error::Error,
    names::ProjectName,
    paths::{self, FileIdentity},
};

use super::{Instance, Project, Registry};

/// The owning project and registered instance, borrowed from the registry.
#[derive(Debug, Clone, Copy)]
pub struct ResolvedInstance<'a> {
    pub project_name: &'a ProjectName,
    pub project: &'a Project,
    pub instance: &'a Instance,
}

/// A per-run index of existing registered folders by device and inode.
///
/// Rebuild after changing the registry or replacing instance directories. The
/// borrowed registry cannot be mutated while this index is in use.
pub struct InstanceResolver<'a> {
    instances: HashMap<FileIdentity, ResolvedInstance<'a>>,
}

impl<'a> InstanceResolver<'a> {
    pub fn new(registry: &'a Registry) -> Result<Self, Error> {
        let mut instances = HashMap::new();
        for (project_name, project) in &registry.projects {
            for instance in &project.instances {
                let identity = match FileIdentity::from_path(&instance.path) {
                    Ok(identity) => identity,
                    Err(Error::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                let resolved = ResolvedInstance {
                    project_name,
                    project,
                    instance,
                };
                if let Some(other) = instances.insert(identity, resolved) {
                    return Err(Error::Registry {
                        message: format!(
                            "Instances {} ({}) and {} ({project_name}) identify the same folder",
                            other.instance.path.display(),
                            other.project_name,
                            instance.path.display(),
                        ),
                        hint: "Remove the duplicate instance from registry.yml before retrying."
                            .into(),
                    });
                }
            }
        }
        Ok(Self { instances })
    }

    /// Find the first registered ancestor, including the folder itself and `/`.
    ///
    /// Canonicalization resolves symlink aliases before walking physical parents.
    /// Relative paths are accepted. An existing unattached folder returns `None`;
    /// missing/inaccessible paths and non-directories return actionable errors.
    pub fn resolve(&self, folder: &Path) -> Result<Option<ResolvedInstance<'a>>, Error> {
        let folder = paths::canonicalize(folder)?;
        let metadata = std::fs::metadata(&folder).map_err(|source| Error::Io {
            operation: format!("Inspect instance folder {}", folder.display()),
            source,
            hint: "Check that the current folder exists and is accessible.".into(),
        })?;
        if !metadata.is_dir() {
            return Err(Error::Usage {
                message: format!(
                    "Instance resolution requires a directory: {}",
                    folder.display()
                ),
                hint: "Run the command from a project folder.".into(),
            });
        }
        for ancestor in folder.ancestors() {
            let identity = FileIdentity::from_path(ancestor)?;
            if let Some(resolved) = self.instances.get(&identity) {
                return Ok(Some(*resolved));
            }
        }
        Ok(None)
    }
}
