use crate::{
    context::Context,
    error::Error,
    linker,
    registry::{Project, resolve::InstanceResolver},
};
use std::io;

pub fn run(context: &mut Context<'_>, keep_links: bool, delete_project: bool) -> Result<(), Error> {
    super::pins::write_locked(context, |context| {
        let mut registry = super::load_registry(context)?;
        let resolver = InstanceResolver::new(&registry)?;
        let found = resolver
            .resolve(&super::attach::cwd()?)?
            .ok_or_else(|| super::attach::usage("Not attached"))?;
        let name = found.project_name.clone();
        let mut instance = found.instance.clone();
        registry
            .projects
            .get_mut(&name)
            .expect("resolved project")
            .instances
            .retain(|i| i.path != instance.path);
        if delete_project && registry.projects[&name].instances.is_empty() {
            registry.projects.remove(&name);
        }
        // Registry first: a failed cleanup must not silently reattach the folder.
        if !context.flags.dry_run {
            registry.save(context.fs.as_ref(), &context.home.registry())?;
        }
        if !keep_links {
            let report = linker::reconcile(
                context.fs.as_ref(),
                &context.home,
                &mut instance,
                &Project::default(),
                &registry.settings,
                linker::Layout::Kinds,
            )?;
            let mut removed: Vec<std::path::PathBuf> = report
                .links
                .iter()
                .filter(|l| l.status == linker::LinkStatus::Removed)
                .map(|l| l.path.clone())
                .collect();
            linker::exclude::update(
                context.fs.as_ref(),
                context.git.as_ref(),
                &instance.path,
                [],
            )?;
            let mut directories = instance.created_dirs.clone();
            directories.sort_by_key(|p| std::cmp::Reverse(p.as_path().components().count()));
            for relative in directories {
                let path = instance.path.join(relative.as_path());
                // Never follow a directory replaced by a symlink after attach.
                if path
                    .strip_prefix(&instance.path)
                    .expect("relative directory")
                    .ancestors()
                    .filter(|p| !p.as_os_str().is_empty())
                    .any(|p| {
                        context
                            .fs
                            .symlink_metadata(&instance.path.join(p))
                            .is_ok_and(|m| m.file_type().is_symlink())
                    })
                {
                    continue;
                }
                if context.flags.dry_run {
                    let entries = match context.fs.read_dir(&path) {
                        Ok(v) => v,
                        Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                        Err(source) => return Err(cleanup_error(&path, source)),
                    };
                    if entries.iter().all(|entry| removed.contains(entry)) {
                        context
                            .fs
                            .remove_dir(&path)
                            .map_err(|e| cleanup_error(&path, e))?;
                        removed.push(path);
                    }
                } else {
                    match context.fs.remove_dir(&path) {
                        Ok(()) => {}
                        Err(e)
                            if matches!(
                                e.kind(),
                                io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty
                            ) => {}
                        Err(source) => return Err(cleanup_error(&path, source)),
                    }
                }
            }
        }
        context.output.result(&serde_json::json!({"project":name,"instance":instance.path,"keep_links":keep_links,"dry_run":context.flags.dry_run}),&format!("{}Detached {} from {name}",if context.flags.dry_run {"Would have "}else{""},instance.path.display())).map_err(super::output_error)
    })
}
fn cleanup_error(path: &std::path::Path, source: io::Error) -> Error {
    Error::Io {
        operation: format!("Clean detached directory {}", path.display()),
        source,
        hint: "The instance is detached; inspect remaining managed paths manually.".into(),
    }
}
